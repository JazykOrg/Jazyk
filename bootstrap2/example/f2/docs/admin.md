# Admin CLI

The Admin CLI is the operator's tool for running Orderly.

## Usage

Operators run `orderly serve --port 8080 --config /etc/orderly/config.toml` to start
the system, and `orderly report --format csv --out /var/reports/daily.csv` for the
daily numbers. The flags `--verbose` and `--quiet` control logging.

## Rules

The Admin CLI shall refuse to start when the configuration file is missing. When an
operator requests a report, the Admin CLI shall include every Order from the selected
period.
