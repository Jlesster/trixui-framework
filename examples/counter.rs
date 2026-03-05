//! counter — minimal trixui demo.
//!
//! Run: `cargo run --example counter`
//!
//!   +  increment
//!   -  decrement
//!   q  quit

use trixui::prelude::*;

struct Counter {
    count: i32,
}

impl App for Counter {
    type Message = ();

    fn update(&mut self, event: Event<()>) -> Cmd<()> {
        if let Event::Key(k) = event {
            match k.code {
                KeyCode::Char('+') | KeyCode::Up => self.count += 1,
                KeyCode::Char('-') | KeyCode::Down => self.count -= 1,
                KeyCode::Char('q') | KeyCode::Esc => return Cmd::quit(),
                _ => {}
            }
        }
        Cmd::none()
    }

    fn view(&self, frame: &mut Frame) {
        // Extract all immutable data before taking the mutable canvas borrow.
        let t = *frame.theme();
        let area = frame.area();
        let cw = frame.cell_w();
        let ch = frame.cell_h();

        let canvas = frame.canvas();

        // Outer border
        let inner = Block::bordered()
            .title(" trixui counter ")
            .border_style(Style::default().fg(t.active_border))
            .style(Style::default().bg(t.pane_bg))
            .top_accent(t.active_border)
            .render(canvas, area, cw, ch, &t);

        // Split into header + body
        let rows = Layout::vertical(vec![
            Constraint::Length(1), // title row
            Constraint::Fill(1),   // spacer
            Constraint::Length(1), // counter row
            Constraint::Fill(1),   // spacer
            Constraint::Length(1), // hint row
        ])
        .split(inner, cw, ch);

        // Header
        Paragraph::new("Counter")
            .style(Style::default().fg(t.active_title).bold())
            .render(canvas, rows[0], cw, ch, &t);

        // Counter value — centred
        let value = format!("{:+}", self.count);
        let val_fg = if self.count > 0 {
            t.bar_accent
        } else if self.count < 0 {
            PixColor::rgb(243, 139, 168)
        } else {
            t.bar_fg
        };
        Paragraph::new(&value)
            .style(Style::default().fg(val_fg).bold())
            .render(canvas, rows[2], cw, ch, &t);

        // Hint
        Paragraph::new("  +/-/↑/↓ to change  q to quit")
            .style(Style::default().fg(t.bar_dim))
            .render(canvas, rows[4], cw, ch, &t);
    }
}

fn main() -> trixui::Result<()> {
    tracing_subscriber::fmt().init();
    Terminal::new(WinitBackend::new()?)?.run(Counter { count: 0 })
}
