use crossterm::{
    ExecutableCommand, QueueableCommand, cursor,
    style::{self, Stylize},
    terminal::{
        self, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
        window_size,
    },
};
use std::{
    io::{self, Write},
    thread::sleep,
    time::Duration,
};

fn main() -> io::Result<()> {
    try2()?;
    // try_termion();
    Ok(())
}
fn try1() -> io::Result<()> {
    let mut stdout = io::stdout();

    stdout.execute(terminal::Clear(terminal::ClearType::All))?;

    for y in 0..10 {
        for x in 0..20 {
            if (y == 0 || y == 10 - 1) || (x == 0 || x == 20 - 1) {
                // in this loop we are more efficient by not flushing the buffer.
                stdout
                    .queue(cursor::MoveTo(x, y))?
                    .queue(style::PrintStyledContent("█".magenta()))?;
            }
        }
    }
    stdout.flush()?;
    Ok(())
}
fn try2() -> io::Result<()> {
    use std::io::{Write, stdout};

    use crossterm::{
        ExecutableCommand, event, execute,
        style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    };
    // enable_raw_mode()?;
    println!("window size: {:#?}", window_size());
    println!("terminal size: {:#?}", terminal::size());
    println!(
        "is_raw_mode_enabled: {:#?}",
        terminal::is_raw_mode_enabled()
    );
    // or using functions
    stdout()
        .execute(EnterAlternateScreen)?
        .execute(SetForegroundColor(Color::Blue))?
        .execute(SetBackgroundColor(Color::Red))?
        .execute(Print("Styled text here.\n"))?
        .execute(Print("Styled text here.\n"))?
        .execute(Print("Styled text here.\n"))?
        .execute(Print("Styled text here.\n"))?;

    sleep(Duration::from_millis(1000));

    stdout()
        .execute(LeaveAlternateScreen)?
        .execute(ResetColor)?;

    // disable_raw_mode()?;
    Ok(())
}

fn try_termion() {
    println!("Size is {:?}", termion::terminal_size().unwrap())
}