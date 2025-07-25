use super::{SearchManager, SearchResult};
use crate::{
    map::{
        map_event::{MapEvent, Layer},
        geometry_collection::{Geometry, Metadata, Style},
    },
    remote::RoutingSender,
};
use std::sync::{mpsc::{Sender, Receiver, channel}, Arc};

/// UI state for the location search component
pub struct SearchUI {
    search_manager: Arc<SearchManager>,
    query: String,
    results: Vec<SearchResult>,
    selected_index: Option<usize>,
    is_searching: bool,
    show_results: bool,
    last_search_query: String,
    input_has_focus: bool,
    search_result_receiver: Receiver<Result<Vec<SearchResult>, String>>,
    search_result_sender: Sender<Result<Vec<SearchResult>, String>>,
    active_search_query: Option<String>,
    last_input_time: f64,
    debounce_delay: f64,
}

impl SearchUI {
    #[must_use] pub fn new(search_manager: SearchManager) -> Self {
        let (search_result_sender, search_result_receiver) = channel();
        Self {
            search_manager: Arc::new(search_manager),
            query: String::new(),
            results: Vec::new(),
            selected_index: None,
            is_searching: false,
            show_results: false,
            last_search_query: String::new(),
            input_has_focus: false,
            search_result_receiver,
            search_result_sender,
            active_search_query: None,
            last_input_time: 0.0,
            debounce_delay: 0.5,
        }
    }
    
    fn check_search_results(&mut self) {
        if let Ok(result) = self.search_result_receiver.try_recv() {
            log::debug!("Received search result from async task");
            self.is_searching = false;
            self.active_search_query = None;
            match result {
                Ok(results) => {
                    log::debug!("Got {} search results", results.len());
                    self.results = results;
                    self.show_results = !self.results.is_empty();
                    if !self.results.is_empty() {
                        self.selected_index = Some(0);
                    }
                }
                Err(e) => {
                    log::error!("Search failed: {e}");
                    self.results.clear();
                    self.show_results = false;
                }
            }
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, map_sender: &RoutingSender) {
        self.check_search_results();
        
        ui.group(|ui| {
            ui.vertical(|ui| {
                let providers = self.search_manager.provider_names();
                let provider_info = format!("Available providers: {}", providers.join(", "));
                
                let response = ui.add_sized(
                    [ui.available_width(), 0.0],
                    egui::TextEdit::singleline(&mut self.query)
                        .hint_text("Enter coordinates (52.5, 13.4) or search for a place...")
                ).on_hover_text(provider_info);
                
                if response.gained_focus() {
                    self.input_has_focus = true;
                    if !self.results.is_empty() && self.query == self.last_search_query {
                        self.show_results = true;
                    }
                } else if response.lost_focus() {
                    self.input_has_focus = false;
                }
                let current_time = ui.input(|i| i.time);
                let query_changed = response.changed();
                let enter_pressed = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                
                if query_changed {
                    self.last_input_time = current_time;
                }
                let should_search = if enter_pressed {
                    // Only search on Enter if no search result is selected
                    !self.query.trim().is_empty() 
                        && self.query != self.last_search_query
                        && self.selected_index.is_none()
                } else { !self.query.trim().is_empty() 
                    && self.query != self.last_search_query && (current_time - self.last_input_time) >= self.debounce_delay };
                
                if !self.query.trim().is_empty() 
                    && self.query != self.last_search_query 
                    && (current_time - self.last_input_time) < self.debounce_delay {
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
                }
                if should_search && !self.is_searching {
                    self.perform_search();
                }
                if self.is_searching {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.small("Searching...");
                    });
                } else if self.query.trim().is_empty() {
                    ui.small("Try: \"Berlin\", \"52.5, 13.4\", or \"52°30'N 13°24'E\"");
                }
                if self.show_results && !self.results.is_empty() {
                    ui.input(|i| {
                        if i.key_pressed(egui::Key::ArrowDown) {
                            self.selected_index = Some(
                                self.selected_index
                                    .map_or(0, |i| (i + 1).min(self.results.len() - 1))
                            );
                        } else if i.key_pressed(egui::Key::ArrowUp) {
                            self.selected_index = Some(
                                self.selected_index
                                    .map_or(self.results.len() - 1, |i| i.saturating_sub(1))
                            );
                        } else if i.key_pressed(egui::Key::Enter) {
                            log::debug!("Enter pressed in search results, selected_index: {:?}", self.selected_index);
                            if let Some(index) = self.selected_index {
                                if index < self.results.len() {
                                    log::debug!("Calling select_result for index: {index}");
                                    self.select_result(index, map_sender);
                                    log::debug!("Returned from select_result");
                                } else {
                                    log::warn!("Selected index {} out of bounds (results len: {})", index, self.results.len());
                                }
                            } else {
                                log::debug!("No result selected when Enter pressed");
                            }
                        } else if i.key_pressed(egui::Key::Escape) {
                            self.show_results = false;
                            self.selected_index = None;
                        }
                    });
                }
                if self.show_results && !self.results.is_empty() && !self.is_searching {
                    self.show_search_results(ui, map_sender);
                }
                if !self.query.is_empty() {
                    ui.horizontal(|ui| {
                        if ui.small_button("Clear").clicked() {
                            self.clear_search();
                        }
                        
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if !self.results.is_empty() {
                                ui.small(format!("{} result{}", 
                                    self.results.len(),
                                    if self.results.len() == 1 { "" } else { "s" }
                                ));
                            }
                        });
                    });
                }
            });
        });
    }
    
    fn show_search_results(&mut self, ui: &mut egui::Ui, map_sender: &RoutingSender) {
        ui.separator();
        ui.strong("Results:");
        
        egui::ScrollArea::vertical()
            .max_height(200.0)
            .show(ui, |ui| {
                let mut clicked_index = None;
                let results_clone = self.results.clone(); // Clone to avoid borrow checker issues
                
                for (index, result) in results_clone.iter().enumerate() {
                    let is_selected = self.selected_index == Some(index);
                    
                    // Result item with hover effect
                    let response = ui.add_sized(
                        [ui.available_width(), 0.0],
                        egui::Button::new(Self::format_result(result))
                            .fill(if is_selected {
                                ui.style().visuals.selection.bg_fill
                            } else {
                                egui::Color32::TRANSPARENT
                            })
                            .stroke(egui::Stroke::NONE)
                    );
                    
                    if response.clicked() {
                        clicked_index = Some(index);
                    }
                    
                    if response.hovered() {
                        self.selected_index = Some(index);
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    
                    // Show coordinate info on hover
                    if response.hovered() {
                        response.on_hover_text(format!(
                            "📍 {:.4}°, {:.4}°\nRelevance: {:.0}%",
                            result.coordinate.lat,
                            result.coordinate.lon,
                            result.relevance * 100.0
                        ));
                    }
                }
                
                // Handle click after the loop to avoid borrowing issues
                if let Some(index) = clicked_index {
                    self.select_result(index, map_sender);
                }
            });
    }
    
    fn format_result(result: &SearchResult) -> String {
        // Truncate long names for better display (using character count, not bytes)
        let name = if result.name.chars().count() > 40 {
            let truncated: String = result.name.chars().take(37).collect();
            format!("{truncated}...")
        } else {
            result.name.clone()
        };
        
        // Add country if available
        match &result.country {
            Some(country) => format!("📍 {name} ({country})"),
            None => format!("📍 {name}"),
        }
    }
    
    fn select_result(&mut self, index: usize, map_sender: &RoutingSender) {
        log::debug!("select_result called with index: {}, results len: {}", index, self.results.len());
        if let Some(result) = self.results.get(index) {
            log::debug!("Found result: {} at {:.4}, {:.4}", result.name, result.coordinate.lat, result.coordinate.lon);
            let style = Style::default()
                .with_color(egui::Color32::RED)
                .with_fill_color(egui::Color32::from_rgba_unmultiplied(255, 0, 0, 100));
                
            let metadata = Metadata::default()
                .with_label(result.name.clone())
                .with_style(style);
            
            let marker_geometry = Geometry::Point(result.coordinate, metadata);
            
            let search_layer = Layer {
                id: "search_results".to_string(),
                geometries: vec![marker_geometry.into()],
            };
            
            if let Err(e) = map_sender.send(MapEvent::Layer(search_layer)) {
                log::error!("Failed to send Layer event: {e}");
            }
            
            log::info!("Sending FocusOn event for: {:.4}, {:.4}", 
                result.coordinate.lat, result.coordinate.lon);
            if let Err(e) = map_sender.send(MapEvent::FocusOn {
                coordinate: result.coordinate,
                zoom_level: Some(16),
            }) {
                log::error!("Failed to send FocusOn event: {e}");
            }
            self.show_results = false;
            self.selected_index = None;
            
            log::info!("Selected location: {} at {:.4}, {:.4}", 
                result.name, result.coordinate.lat, result.coordinate.lon);
        }
    }
    
    fn perform_search(&mut self) {
        let query = self.query.trim().to_string();
        if query.is_empty() {
            return;
        }
        
        if self.is_searching || query == self.last_search_query {
            return;
        }
        
        self.last_search_query.clone_from(&query);
        self.show_results = false;
        self.selected_index = None;
        
        if let Some(active_query) = &self.active_search_query {
            if active_query == &query {
                log::debug!("Search already in progress for: {query}");
                return;
            }
        }
        
        log::debug!("Starting async search for: {query}");
        self.is_searching = true;
        self.active_search_query = Some(query.clone());
        let search_manager = Arc::clone(&self.search_manager);
        let sender = self.search_result_sender.clone();
        let query_for_task = query.clone();
        
        tokio::spawn(async move {
            let result = match tokio::time::timeout(
                std::time::Duration::from_secs(10),
                search_manager.search(&query_for_task)
            ).await {
                Ok(Ok(results)) => Ok(results),
                Ok(Err(e)) => Err(e.to_string()),
                Err(_) => Err("Search timed out".to_string()),
            };
            
            let _ = sender.send(result);
        });
    }
    
    fn clear_search(&mut self) {
        self.query.clear();
        self.results.clear();
        self.show_results = false;
        self.selected_index = None;
        self.is_searching = false;
        self.last_search_query.clear();
        self.input_has_focus = false;
        self.active_search_query = None;
    }
    
    
}

