//! Port of CliArgs.cs — same flags so installers/backend-generated commands keep working.

#[derive(Debug, Default, Clone, PartialEq)]
pub struct CliArgs {
    pub data_dir: Option<String>,
    pub config_path: Option<String>,
    pub enroll_token: Option<String>,
    pub endpoint: Option<String>,
    pub inventory_seconds: Option<i32>,
    pub print_config: bool,
    pub print_fingerprint: bool,
    pub print_inventory: bool,
    pub print_security: bool,
    pub print_about: bool,
    pub print_version: bool,
    pub once: bool,
    pub send_inventory: bool,
    pub check_update: bool,
    pub update_now: bool,
    pub show_help: bool,
}

impl CliArgs {
    pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Self {
        let mut cli = Self::default();
        let mut it = args.into_iter();
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--data-dir" => cli.data_dir = it.next(),
                "--config" => cli.config_path = it.next(),
                "--enroll-token" => cli.enroll_token = it.next(),
                "--endpoint" => cli.endpoint = it.next(),
                "--inventory-seconds" | "--inventory-interval" => {
                    if let Some(sec) = it.next().and_then(|v| v.trim().parse().ok()) {
                        cli.inventory_seconds = Some(sec);
                    }
                }
                "--print-config" => cli.print_config = true,
                "--print-fingerprint" => cli.print_fingerprint = true,
                "--print-inventory" => cli.print_inventory = true,
                "--print-security" => cli.print_security = true,
                "--about" | "--info" => cli.print_about = true,
                "--version" | "-v" => cli.print_version = true,
                "--once" => cli.once = true,
                "--send-inventory" => cli.send_inventory = true,
                "--check-update" => cli.check_update = true,
                "--update-now" => cli.update_now = true,
                "--help" | "-h" => cli.show_help = true,
                other => eprintln!("[warn] Bỏ qua tham số không biết: {other}"),
            }
        }
        cli
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(a: &[&str]) -> CliArgs {
        CliArgs::parse(a.iter().map(|s| s.to_string()))
    }

    #[test]
    fn parses_all_flags() {
        let c = p(&[
            "--data-dir",
            "/d",
            "--config",
            "/c.json",
            "--enroll-token",
            "t",
            "--endpoint",
            "https://x",
            "--inventory-interval",
            "60",
            "--once",
            "-v",
            "--info",
        ]);
        assert_eq!(c.data_dir.as_deref(), Some("/d"));
        assert_eq!(c.config_path.as_deref(), Some("/c.json"));
        assert_eq!(c.enroll_token.as_deref(), Some("t"));
        assert_eq!(c.endpoint.as_deref(), Some("https://x"));
        assert_eq!(c.inventory_seconds, Some(60));
        assert!(c.once && c.print_version && c.print_about);
    }

    #[test]
    fn ignores_unknown_and_bad_numbers() {
        let c = p(&["--bogus", "--inventory-seconds", "abc", "--check-update"]);
        assert_eq!(c.inventory_seconds, None);
        assert!(c.check_update);
    }
}
