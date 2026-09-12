use chrono::{DateTime, Utc};
use product::admin_cli::{AdminCli, Order, REPORT_COMMAND, SERVE_COMMAND};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorRunbook {
    pub tool_name: &'static str,
    pub serves_system: &'static str,
}

impl OperatorRunbook {
    // req:req:admin-1 hash:e8928a4d
    pub fn admin_cli_tool() -> Self {
        Self {
            tool_name: "Admin CLI",
            serves_system: "Orderly",
        }
    }

    // req:req:system-8 hash:9d522f0b
    pub fn management_tool_name() -> &'static str {
        "Admin CLI"
    }

    // req:req:admin-2 hash:1683a24d
    pub fn serve_command_to_start_orderly() -> &'static str {
        SERVE_COMMAND
    }

    pub fn serve_port_and_config() -> (u16, PathBuf) {
        let invocation = AdminCli::serve_invocation();
        (invocation.port, invocation.config)
    }

    // req:req:admin-3 hash:a9cc3499
    pub fn daily_numbers_report_command() -> &'static str {
        REPORT_COMMAND
    }

    pub fn report_format_and_output() -> (String, PathBuf) {
        let invocation = AdminCli::report_invocation();
        (invocation.format, invocation.out)
    }

    // req:req:admin-6 hash:2c3c595c
    pub fn requested_report(cli: &AdminCli, start: DateTime<Utc>, end: DateTime<Utc>) -> Vec<Order> {
        cli.generate_report(start, end)
    }
}
