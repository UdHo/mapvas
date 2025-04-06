use std::marker::PhantomData;

use super::XY;

/// A strongly typed transform, mean to be used between ``PixelCoordinates`` (=Coordinates) and
/// ``PixelPosition`` (=pixel in the UI).
#[derive(Debug, PartialEq, Copy, Clone)]
pub struct TTransform<F: XY, T: XY> {
  pub zoom: f32,
  pub trans: T,
  phantom_data: PhantomData<F>,
}

impl<F: XY, T: XY + Default> Default for TTransform<F, T> {
  fn default() -> Self {
    Self {
      zoom: 1.,
      trans: T::default(),
      phantom_data: PhantomData,
    }
  }
}

/// We want to avoid accidental conversions between incompatible coordinates.
pub trait PrivateInto<T> {
  fn conv(self) -> T;
}

impl<F: XY, T: XY> PrivateInto<T> for F {
  fn conv(self) -> T {
    T::default().with_x(self.x()).with_y(self.y())
  }
}

impl<F: XY, T: XY> TTransform<F, T>
where
  F: PrivateInto<T>,
  T: PrivateInto<F>,
{
  /// Checks if the transform is invalid.
  #[must_use]
  pub fn is_invalid(&self) -> bool {
    !self.zoom.is_nan() && !self.trans.x().is_nan() && !self.trans.y().is_nan() && self.zoom == 0.
  }

  //// Zooms the transform.
  #[must_use]
  pub fn zoomed(mut self, factor: f32) -> Self {
    self.zoom *= factor;
    self
  }

  /// Zooms the transform.
  pub fn zoom(&mut self, factor: f32) -> &mut Self {
    self.zoom *= factor;
    self
  }

  /// Translates.
  pub fn translate(&mut self, delta: T) -> &mut Self {
    self.trans += delta;
    self
  }

  /// Translates.
  #[must_use]
  pub fn translated(mut self, delta: T) -> Self {
    self.translate(delta);
    self
  }

  /// Chains transforms.
  #[must_use]
  pub fn and_then_transform(&self, transform: &TTransform<T, T>) -> Self {
    Self {
      zoom: self.zoom * transform.zoom,
      trans: self.trans * transform.zoom + transform.trans,
      phantom_data: PhantomData,
    }
  }

  /// The inverse ```TTransform```.
  #[must_use]
  pub fn invert(self) -> TTransform<T, F> {
    TTransform {
      zoom: 1. / self.zoom,
      trans: self.trans.conv() * (-1. / self.zoom),
      phantom_data: PhantomData,
    }
  }

  /// Applies the transform to a coordinate.
  pub fn apply(&self, from: F) -> T {
    (from * self.zoom).conv() + self.trans
  }

  /// Returns an invalid transform.
  #[must_use]
  pub fn invalid() -> Self {
    Self {
      zoom: 0.,
      trans: T::default(),
      phantom_data: PhantomData,
    }
  }
}

#[cfg(test)]
mod tests {
  use crate::map::coordinates::PixelPosition;

  use super::*;
  use assert_approx_eq::assert_approx_eq;
  #[test]
  fn transform() {
    let trans = TTransform::<PixelPosition, PixelPosition>::default()
      .zoomed(5.)
      .translated(PixelPosition { x: 10., y: 20. });
    let inv = trans.invert();

    assert_approx_eq!(trans.zoom, 5.);
    assert_approx_eq!(trans.trans.x(), 10.);
    assert_approx_eq!(trans.trans.y(), 20.);

    assert_approx_eq!(inv.zoom, 1. / 5.);
    assert_approx_eq!(inv.trans.x, -2.);
    assert_approx_eq!(inv.trans.y, -4.);

    let prod = trans.and_then_transform(&inv);
    assert_approx_eq!(prod.zoom, 1.);
    assert_approx_eq!(prod.trans.x, 0.);
    assert_approx_eq!(prod.trans.y, 0.);

    let prod = inv.and_then_transform(&trans);
    assert_approx_eq!(prod.zoom, 1.);
    assert_approx_eq!(prod.trans.x, 0.);
    assert_approx_eq!(prod.trans.y, 0.);
  }
}
