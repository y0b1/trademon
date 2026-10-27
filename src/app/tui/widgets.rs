//! Candlestick and gauge widgets.
//!
//! The gauge is drawn here rather than with ratatui's `Gauge`, which with an empty label still
//! paints a reversed blank cell in the middle of its middle row.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use super::theme::{gradient, trend};

/// Filled cell of a gauge, and of a candle body.
const FULL: char = '█';
const LOWER_HALF: char = '▄';
const UPPER_HALF: char = '▀';
const WICK: char = '│';

/// Gauge view of the original macmon: a bar across its whole area, filled from the left to `ratio`
/// (rounded to whole cells) in the load color of `ratio`; the rest stays blank.
pub(super) struct Gauge {
  ratio: f64,
}

impl Gauge {
  /// Gauge for `ratio` in `0.0..=1.0`, clamped outside the range.
  pub(super) fn new(ratio: f64) -> Self {
    Self { ratio: if ratio.is_nan() { 0.0 } else { ratio.clamp(0.0, 1.0) } }
  }
}

impl Widget for Gauge {
  fn render(self, area: Rect, buf: &mut Buffer) {
    let area = area.intersection(buf.area);
    // at most `area.width`, as the ratio is at most 1
    let filled = (f64::from(area.width) * self.ratio).round() as u16;
    let color = gradient(self.ratio);
    for y in area.top()..area.bottom() {
      for x in area.left()..area.left() + filled {
        buf[(x, y)].set_char(FULL).set_fg(color);
      }
    }
  }
}

/// Samples in one candle: open is the oldest, close the newest, the wick spans their range.
pub(super) const CANDLE_SAMPLES: usize = 4;

/// Candlestick chart of newest-first samples over its whole area, newest candle on the right, one
/// column per candle in half-row steps: the body (`█` `▀` `▄`) from open to close, the wick (`│`)
/// from low to high; up candles (close ≥ open) in the up color, down candles in the down color.
pub(super) struct Candles<'a> {
  data: &'a [u64],
  max: Option<u64>,
  invert: bool,
}

impl<'a> Candles<'a> {
  /// Chart scaled to its largest visible sample, green up and red down.
  pub(super) fn new(data: &'a [u64]) -> Self {
    Self { data, max: None, invert: false }
  }

  /// Value drawn at full height.
  pub(super) fn max(mut self, max: u64) -> Self {
    self.max = Some(max);
    self
  }

  /// Red up and green down.
  pub(super) fn invert(mut self, invert: bool) -> Self {
    self.invert = invert;
    self
  }
}

impl Widget for Candles<'_> {
  fn render(self, area: Rect, buf: &mut Buffer) {
    let area = area.intersection(buf.area);
    if area.is_empty() {
      return;
    }

    let candles: Vec<&[u64]> =
      self.data.chunks(CANDLE_SAMPLES).take(usize::from(area.width)).collect();
    let max = self.max.unwrap_or_else(|| candles.concat().into_iter().max().unwrap_or(0)).max(1);
    let halves = u64::from(area.height) * 2;
    // the top value lands in the top half row, not past it
    let level = |v: u64| (u128::from(v.min(max)) * u128::from(halves - 1) / u128::from(max)) as u64;
    for (x, chunk) in (area.left()..area.right()).rev().zip(candles) {
      let (close, open) = (chunk[0], chunk[chunk.len() - 1]);
      let (low, high) = chunk.iter().fold((u64::MAX, 0), |(l, h), &v| (l.min(v), h.max(v)));
      let color = trend(close >= open, self.invert);
      // half rows from the bottom: the body covers `bottom..=top`, the wick `wick_low..=wick_high`
      let (bottom, top) = (level(open.min(close)), level(open.max(close)));
      let (wick_low, wick_high) = (level(low), level(high));
      for (row, y) in (area.top()..area.bottom()).rev().enumerate() {
        let (lower, upper) = (row as u64 * 2, row as u64 * 2 + 1);
        let body = |h: u64| (bottom..=top).contains(&h);
        let symbol = match (body(lower), body(upper)) {
          (true, true) => FULL,
          (true, false) => LOWER_HALF,
          (false, true) => UPPER_HALF,
          _ if wick_low <= upper && lower <= wick_high => WICK,
          _ => continue,
        };
        buf[(x, y)].set_char(symbol).set_fg(color);
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use ratatui::buffer::Buffer;
  use ratatui::layout::Rect;
  use ratatui::style::Color;
  use ratatui::widgets::Widget;

  use super::{Candles, Gauge};

  fn draw(widget: impl Widget, width: u16, height: u16) -> Buffer {
    let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
    widget.render(buf.area, &mut buf);
    buf
  }

  fn rows(buf: &Buffer) -> Vec<String> {
    let row = |y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect();
    (0..buf.area.height).map(row).collect()
  }

  #[test]
  fn candles_group_samples_with_body_and_wick() {
    // newest first, two rows (four halves) at max 100: the right candle opens at 0 and closes at
    // 100 (up, full body); the left one opens at 100, closes at 50 with a dip to 0 (down, body in
    // the upper three halves, the wick hidden under it)
    let data = [100, 50, 50, 0, 50, 0, 100, 100];
    let buf = draw(Candles::new(&data).max(100), 2, 2);
    assert_eq!(rows(&buf), ["██", "▀█"]);
    assert_eq!((buf[(0, 0)].fg, buf[(1, 0)].fg), (Color::Red, Color::Green));
    // inverted: up red, down green
    let buf = draw(Candles::new(&data).max(100).invert(true), 2, 2);
    assert_eq!((buf[(0, 1)].fg, buf[(1, 1)].fg), (Color::Green, Color::Red));
    // a flat candle is a half cell; without a maximum scaled to the largest visible sample
    assert_eq!(rows(&draw(Candles::new(&[10, 10, 10, 10]), 1, 2)), ["▀", " "]);
  }

  #[test]
  fn gauge_fills_its_ratio_of_every_row() {
    // (ratio, width, filled cells): rounded to whole cells, clamped to the range
    let cases =
      [(0.0, 10, 0), (0.5, 10, 5), (0.556, 48, 27), (0.04, 10, 0), (1.5, 4, 4), (f64::NAN, 4, 0)];
    for (ratio, width, filled) in cases {
      let buf = draw(Gauge::new(ratio), width, 3);
      let row = format!("{}{}", "█".repeat(filled), " ".repeat(usize::from(width) - filled));
      assert_eq!(rows(&buf), vec![row; 3], "{ratio} in {width}");
    }
    // in the load color of the ratio
    assert_eq!(draw(Gauge::new(0.5), 4, 1)[(0, 0)].fg, Color::Yellow);
  }
}
