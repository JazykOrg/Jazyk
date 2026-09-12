use chrono::{Duration, Utc};
use product::admin_cli::{AdminCli, Order};
use std::fs;
use std::path::{Path, PathBuf};

#[path = "../src/operators.rs"]
mod operators;

use operators::OperatorRunbook;

fn write_config(path: &Path) {
    fs::write(path, "database=sqlite\n").expect("test config should be writable");
}

fn remove_if_exists(path: &Path) {
    let _ = fs::remove_file(path);
}

// req:req:admin-1 hash:e8928a4d
#[test]
fn req_admin_1_e8928a4d() {
    let runbook = OperatorRunbook::admin_cli_tool();

    assert_eq!(runbook.tool_name, "Admin CLI");
    assert_eq!(runbook.serves_system, "Orderly");
}

// req:req:admin-2 hash:1683a24d
#[test]
fn req_admin_2_1683a24d() {
    let (port, config) = OperatorRunbook::serve_port_and_config();

    assert_eq!(OperatorRunbook::serve_command_to_start_orderly(), "orderly serve --port 8080 --config /etc/orderly/config.toml");
    assert_eq!(port, 8080);
    assert_eq!(config, PathBuf::from("/etc/orderly/config.toml"));
}

// req:req:admin-3 hash:a9cc3499
#[test]
fn req_admin_3_a9cc3499() {
    let (format, out) = OperatorRunbook::report_format_and_output();

    assert_eq!(OperatorRunbook::daily_numbers_report_command(), "orderly report --format csv --out /var/reports/daily.csv");
    assert_eq!(format, "csv");
    assert_eq!(out, PathBuf::from("/var/reports/daily.csv"));
}

// req:req:admin-6 hash:2c3c595c
#[test]
fn req_admin_6_2c3c595c() {
    let temp_config_path = PathBuf::from("temp_operators_req_admin_6.toml");
    write_config(&temp_config_path);
    let now = Utc::now();
    let selected_start = now - Duration::days(7);
    let selected_end = now;
    let expected_inside_ids = vec![10, 11, 12];
    let orders = vec![
        Order { id: 9, placement_time: selected_start - Duration::seconds(1), total_amount: 9.0 },
        Order { id: 10, placement_time: selected_start, total_amount: 10.0 },
        Order { id: 11, placement_time: now - Duration::days(3), total_amount: 11.0 },
        Order { id: 12, placement_time: selected_end, total_amount: 12.0 },
        Order { id: 13, placement_time: selected_end + Duration::seconds(1), total_amount: 13.0 },
    ];
    let cli = AdminCli::with_orders(&temp_config_path, orders).expect("valid config should construct CLI");

    let report = OperatorRunbook::requested_report(&cli, selected_start, selected_end);

    let reported_ids: Vec<u32> = report.iter().map(|order| order.id).collect();
    assert_eq!(reported_ids, expected_inside_ids);
    assert!(report.iter().all(|order| order.placement_time >= selected_start && order.placement_time <= selected_end));

    remove_if_exists(&temp_config_path);
}

// req:req:system-8 hash:9d522f0b
#[test]
fn req_system_8_9d522f0b() {
    let temp_config_path = PathBuf::from("temp_operators_req_system_8.toml");
    write_config(&temp_config_path);
    let cli = AdminCli::new(&temp_config_path).expect("valid config should construct CLI");

    assert_eq!(OperatorRunbook::management_tool_name(), "Admin CLI");
    assert_eq!(cli.management_interface_name(), OperatorRunbook::management_tool_name());
    assert_eq!(OperatorRunbook::serve_command_to_start_orderly(), "orderly serve --port 8080 --config /etc/orderly/config.toml");
    assert_eq!(OperatorRunbook::daily_numbers_report_command(), "orderly report --format csv --out /var/reports/daily.csv");

    remove_if_exists(&temp_config_path);
}
