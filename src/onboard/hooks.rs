use anyhow::Result;
use dialoguer::{Confirm, Input};
use ryvos_core::config::HooksConfig;

pub fn configure() -> Result<Option<HooksConfig>> {
    let enable = Confirm::new()
        .with_prompt("Configure lifecycle hooks?")
        .default(false)
        .interact()?;

    if !enable {
        return Ok(None);
    }

    println!();
    println!("  Hooks run shell commands on lifecycle events.");
    println!("  Available env vars: $RYVOS_SESSION, $RYVOS_TEXT, $RYVOS_TOOL");
    println!();

    let on_start = prompt_hook("on_start (daemon/REPL starts)")?;
    let on_message = prompt_hook("on_message (message received)")?;
    let on_tool_call = prompt_hook("on_tool_call (tool invoked)")?;
    let on_response = prompt_hook("on_response (response sent)")?;

    let hooks = HooksConfig {
        on_start,
        on_message,
        on_tool_call,
        on_response,
        on_turn_complete: vec![],
        on_tool_error: vec![],
        on_session_start: vec![],
        on_session_end: vec![],
    };

    if hooks.is_empty() {
        return Ok(None);
    }

    Ok(Some(hooks))
}

fn prompt_hook(label: &str) -> Result<Vec<String>> {
    let cmd: String = Input::new()
        .with_prompt(format!("{label} command (blank to skip)"))
        .allow_empty(true)
        .interact_text()?;

    Ok(if cmd.is_empty() { vec![] } else { vec![cmd] })
}
