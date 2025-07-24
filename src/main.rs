use bad_editor::bad;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // TODO: CLI
    let app = bad::App::new();

    crossterm::terminal::enable_raw_mode()?;
    let result = app.run(&mut std::io::stdout());
    crossterm::terminal::disable_raw_mode()?;

    result
}
