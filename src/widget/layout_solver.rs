//! layout_solver.rs — constraint-based layout engine (ratatui Flex style).

use crate::layout::Rect;

/// Column or row size constraint.
///
/// # Unit notes
///
/// - `Fixed` is in **pixels** — exact screen pixels, font-independent.
/// - `Length`, `Min`, `Max` are in **cells** — multiplied by `cell_w` or
///   `cell_h` internally. Don't mix up pixel and cell values.
/// - `Percentage`, `Ratio`, `Fill` are relative — no unit ambiguity.
#[derive(Debug, Clone, Copy)]
pub enum Constraint {
    /// Exact pixel size.
    Fixed(u32),
    /// Exactly `n` cells (multiplied by `cell_w` / `cell_h`).
    Length(u32),
    /// Percentage of the available space (0–100).
    Percentage(u8),
    /// Fraction `a/b` of the available space.
    Ratio(u32, u32),
    /// At least `n` cells.
    Min(u32),
    /// At most `n` cells.
    Max(u32),
    /// Fill remaining space weighted by this value. `Fill(2)` gets twice
    /// the remaining space of `Fill(1)`.
    Fill(u32),
}

/// How children are distributed within any leftover space.
///
/// `Stretch` is an alias for `Start` — fill-weight children absorb all
/// slack; the flex mode only affects items without fill weight.
#[derive(Debug, Clone, Copy, Default)]
pub enum Flex {
    /// Pack to the start (default).
    #[default]
    Start,
    /// Centre the group.
    Center,
    /// Pack to the end.
    End,
    /// Distribute slack evenly between items.
    SpaceBetween,
    /// Distribute slack evenly around items.
    SpaceAround,
    /// Alias for `Start`. Named for API parity with CSS flexbox.
    Stretch,
}

#[derive(Debug, Clone, Copy)]
pub enum Direction {
    Horizontal,
    Vertical,
}

pub struct Layout {
    direction: Direction,
    constraints: Vec<Constraint>,
    flex: Flex,
    spacing: u32,
}

impl Layout {
    pub fn horizontal(c: impl Into<Vec<Constraint>>) -> Self {
        Self {
            direction: Direction::Horizontal,
            constraints: c.into(),
            flex: Flex::default(),
            spacing: 0,
        }
    }
    pub fn vertical(c: impl Into<Vec<Constraint>>) -> Self {
        Self {
            direction: Direction::Vertical,
            constraints: c.into(),
            flex: Flex::default(),
            spacing: 0,
        }
    }
    pub fn flex(mut self, f: Flex) -> Self {
        self.flex = f;
        self
    }
    pub fn spacing(mut self, px: u32) -> Self {
        self.spacing = px;
        self
    }

    pub fn split(self, area: Rect, cell_w: u32, cell_h: u32) -> Vec<Rect> {
        let n = self.constraints.len();
        if n == 0 {
            return vec![];
        }

        let total = match self.direction {
            Direction::Horizontal => area.w,
            Direction::Vertical => area.h,
        };
        let unit = match self.direction {
            Direction::Horizontal => cell_w,
            Direction::Vertical => cell_h,
        };

        let spacing_total = self.spacing.saturating_mul((n as u32).saturating_sub(1));
        let available = total.saturating_sub(spacing_total);

        let mut sizes: Vec<Option<u32>> = vec![None; n];
        let mut allocated: u32 = 0;
        let mut fill_weight: u32 = 0;

        for (i, c) in self.constraints.iter().enumerate() {
            let sz = match *c {
                Constraint::Fixed(px) => Some(px),
                Constraint::Length(cells) => Some(cells * unit),
                Constraint::Percentage(p) => Some((available as f32 * p as f32 / 100.0) as u32),
                Constraint::Ratio(a, b) => Some(if b == 0 {
                    0
                } else {
                    (available as f32 * a as f32 / b as f32) as u32
                }),
                Constraint::Min(cells) => Some(cells * unit),
                Constraint::Max(cells) => Some(cells * unit),
                Constraint::Fill(_) => None,
            };
            if let Some(s) = sz {
                allocated += s;
            } else if let Constraint::Fill(w) = *c {
                fill_weight += w;
            }
            sizes[i] = sz;
        }

        let remaining = available.saturating_sub(allocated);
        if fill_weight > 0 {
            for (i, c) in self.constraints.iter().enumerate() {
                if let Constraint::Fill(w) = *c {
                    sizes[i] = Some((remaining as f32 * w as f32 / fill_weight as f32) as u32);
                }
            }
        }

        let mut sizes: Vec<u32> = sizes.into_iter().map(|s| s.unwrap_or(0)).collect();
        for (i, c) in self.constraints.iter().enumerate() {
            if let Constraint::Max(cells) = *c {
                sizes[i] = sizes[i].min(cells * unit);
            }
        }

        let used = sizes.iter().sum::<u32>() + spacing_total;
        let slack = total.saturating_sub(used);

        let mut offsets: Vec<u32> = Vec::with_capacity(n);
        match self.flex {
            Flex::Start | Flex::Stretch => {
                let mut cur = 0u32;
                for (i, &sz) in sizes.iter().enumerate() {
                    offsets.push(cur);
                    cur += sz + if i + 1 < n { self.spacing } else { 0 };
                }
            }
            Flex::End => {
                let mut cur = slack;
                for (i, &sz) in sizes.iter().enumerate() {
                    offsets.push(cur);
                    cur += sz + if i + 1 < n { self.spacing } else { 0 };
                }
            }
            Flex::Center => {
                let mut cur = slack / 2;
                for (i, &sz) in sizes.iter().enumerate() {
                    offsets.push(cur);
                    cur += sz + if i + 1 < n { self.spacing } else { 0 };
                }
            }
            Flex::SpaceBetween => {
                let gap = if n > 1 { slack / (n as u32 - 1) } else { 0 };
                let mut cur = 0u32;
                for (i, &sz) in sizes.iter().enumerate() {
                    offsets.push(cur);
                    cur += sz + gap + if i + 1 < n { self.spacing } else { 0 };
                }
            }
            Flex::SpaceAround => {
                let gap = slack / (n as u32 + 1);
                let mut cur = gap;
                for (i, &sz) in sizes.iter().enumerate() {
                    offsets.push(cur);
                    cur += sz + gap + if i + 1 < n { self.spacing } else { 0 };
                }
            }
        }

        offsets
            .iter()
            .zip(sizes.iter())
            .enumerate()
            .map(|(i, (&off, &sz))| {
                let is_last = i + 1 == n;
                match self.direction {
                    Direction::Horizontal => {
                        let w = if is_last {
                            total.saturating_sub(off)
                        } else {
                            sz
                        };
                        Rect {
                            x: area.x + off,
                            y: area.y,
                            w,
                            h: area.h,
                        }
                    }
                    Direction::Vertical => {
                        let h = if is_last {
                            total.saturating_sub(off)
                        } else {
                            sz
                        };
                        Rect {
                            x: area.x,
                            y: area.y + off,
                            w: area.w,
                            h,
                        }
                    }
                }
            })
            .collect()
    }
}
