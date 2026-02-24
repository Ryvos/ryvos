pub fn print_banner() {
    let version = env!("CARGO_PKG_VERSION");
    match tui_banner::Banner::new("RYVOS")
        .map(|b| b.style(tui_banner::Style::NeonCyber).render())
    {
        Ok(banner) => {
            println!("{banner}");
            println!("             Ryvos v{version}");
            println!("        Blazingly fast AI agent runtime\n");
        }
        Err(_) => {
            println!("\n  RYVOS v{version}\n  Blazingly fast AI agent runtime\n");
        }
    }
}
