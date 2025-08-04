use color_eyre::Result;
use crossterm::{
    event::{self, Event},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    DefaultTerminal, Frame, Terminal, layout::Rect, prelude::CrosstermBackend, widgets::Paragraph,
};

fn main() {
    ratatui_helloworld();
    // rata2();
}


fn rata2() -> Result<()> {
    use std::{
        io::{Result, stderr, stdout},
        thread::sleep,
        time::Duration,
    };

    use ratatui::crossterm::{
        ExecutableCommand,
        terminal::{EnterAlternateScreen, LeaveAlternateScreen},
    };
    let should_enter_alternate_screen = std::env::args().nth(1).unwrap().parse::<bool>().unwrap();
    if should_enter_alternate_screen {
        stdout().execute(EnterAlternateScreen)?;
    }

    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    // enable_raw_mode()?;
    // disable_raw_mode()?;


    terminal.draw(|f| {
        println!("current area is {:#?}", f.area());
        f.render_widget(Paragraph::new("Hello World!"), Rect::new(10, 10, 20, 1));
    })?;
    sleep(Duration::from_secs(2));

    if should_enter_alternate_screen {
        stdout().execute(LeaveAlternateScreen)?;
    }
    // disable_raw_mode()?;
    Ok(())
}


fn render_demo_scrollbar(frame: &mut Frame) {
    use ratatui::{
        Frame,
        layout::{Margin, Rect},
        text::Line,
        widgets::{
            Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
            StatefulWidget,
        },
    };

    let vertical_scroll = 0; // from app state

    let items = vec![
        Line::from("Item 1"),
        Line::from("Item 2"),
        Line::from("Item 3"),
    ];
    let paragraph = Paragraph::new(items.clone())
        .scroll((vertical_scroll as u16, 0))
        .block(Block::new().borders(Borders::RIGHT)); // to show a background for the scrollbar

    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(Some("↑"))
        .end_symbol(Some("↓"));

    let mut scrollbar_state = ScrollbarState::new(items.len()).position(vertical_scroll);

    let area = frame.area();
    // Note we render the paragraph
    frame.render_widget(paragraph, area);
    // and the scrollbar, those are separate widgets
    frame.render_stateful_widget(
        scrollbar,
        area.inner(Margin {
            // using an inner vertical margin of 1 unit makes the scrollbar inside the block
            vertical: 1,
            horizontal: 0,
        }),
        &mut scrollbar_state,
    );
}
fn ratatui_helloworld() -> Result<()> {
    color_eyre::install()?;
    let terminal = ratatui::init();
    let result = run(terminal);
    ratatui::restore();
    result
}

fn run(mut terminal: DefaultTerminal) -> Result<()> {
    loop {
        // terminal.draw(render)?;
        terminal.draw(render_demo_scrollbar)?;
        if matches!(event::read()?, Event::Key(_)) {
            break Ok(());
        }
    }
}

fn render(frame: &mut Frame) {
    frame.render_widget("hello world", frame.area());
}
