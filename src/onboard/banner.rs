pub fn print_banner() {
    let version = env!("CARGO_PKG_VERSION");
    println!(
        r#"
    ____             __  ________
   / __ \__  _______/ /_/ ____/ /___ __      __
  / /_/ / / / / ___/ __/ /   / / __ `/ | /| / /
 / _, _/ /_/ (__  ) /_/ /___/ / /_/ /| |/ |/ /
/_/ |_|\__,_/____/\__/\____/_/\__,_/ |__/|__/

             Ryvos v{version}
        Blazingly fast AI agent runtime
"#
    );
}
