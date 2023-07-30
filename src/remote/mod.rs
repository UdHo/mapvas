#[derive(serde::Serialize, serde::Deserialize)]
pub enum Query {}
pub async fn serve_axum(State(i): State<i32>, Json(_): Json<Query>) -> String {
  i.into()
}
