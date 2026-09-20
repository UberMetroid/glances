//! `--export <name>` → exporter dispatch. Split from `mod.rs` for the
//! 256-line cap.

use crate::cli::args::Args;
use crate::core::error::{GlancesError, Result};
use crate::core::value::Value;
use crate::exports::*;

/// Look up the value of `--export-<key>` (e.g. `opt("mqtt-server")`).
/// Last occurrence wins — documented "later flags override" semantics.
pub(crate) fn opt<'a>(args: &'a Args, key: &str) -> Option<&'a str> {
    args.export_opts.iter().rfind(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

/// Parse `host[:port]` / `[v6host]:port` — bare host keeps `default_port`.
/// Bracketed IPv6 keeps its brackets stripped; a bare multi-colon string
/// without brackets is treated as a portless IPv6 literal.
fn split_host_port(s: &str, default_port: u16) -> (String, u16) {
    if let Some(rest) = s.strip_prefix('[') {
        if let Some((h, tail)) = rest.split_once(']') {
            let port = tail.strip_prefix(':')
                .and_then(|p| p.parse().ok())
                .unwrap_or(default_port);
            return (h.to_string(), port);
        }
    }
    match s.rsplit_once(':') {
        // Exactly one colon → host:port. More → bare IPv6 literal.
        Some((h, p)) if !h.is_empty() && !h.contains(':') => {
            (h.to_string(), p.parse().unwrap_or(default_port))
        }
        _ => (s.to_string(), default_port),
    }
}

/// Build the host/port pair for `key`, falling back to the Config default.
fn hp(args: &Args, key: &str, default_port: u16, default_host: &str) -> (String, u16) {
    opt(args, key).map(|s| split_host_port(s, default_port))
        .unwrap_or_else(|| (default_host.to_string(), default_port))
}

pub(crate) fn hp_into(args: &Args, key: &str, host: &mut String, port: &mut u16) {
    let (h, p) = hp(args, key, *port, host);
    *host = h; *port = p;
}

pub(crate) fn set_str(args: &Args, key: &str, dst: &mut String) {
    if let Some(v) = opt(args, key) { *dst = v.to_string(); }
}

fn set_opt(args: &Args, key: &str, dst: &mut Option<String>) {
    if let Some(v) = opt(args, key) { *dst = Some(v.to_string()); }
}

pub(crate) fn dispatch(name: &str, snap: &Value, flat: &[flatten::Field<'_>], args: &Args) -> Result<()> {
    if let Some(r) = super::extra::dispatch(name, flat, args) {
        return r;
    }
    match name {
        n if n == csv::NAME => {
            let mut c = csv::Config::default();
            set_str(args, "csv-file", &mut c.path);
            c.overwrite = args.export_csv_overwrite;
            csv::write(flat, &c)
        }
        n if n == json::NAME => {
            let mut c = json::Config::default();
            set_str(args, "json-file", &mut c.path);
            json::write(snap, &c)
        }
        n if n == influxdb::NAME => {
            let mut c = influxdb::Config::default();
            hp_into(args, "influxdb-host", &mut c.host, &mut c.port);
            set_str(args, "influxdb-db", &mut c.database);
            set_str(args, "influxdb-prefix", &mut c.prefix);
            set_opt(args, "influxdb-file", &mut c.file);
            set_opt(args, "influxdb-user", &mut c.user);
            set_opt(args, "influxdb-password", &mut c.password);
            if let Some(t) = opt(args, "influxdb-tags") {
                c.tags = influxdb::parse_tags(&t);
            }
            if let Some(h) = snap
                .as_object()
                .and_then(|o| o.get("system"))
                .and_then(|v| v.as_object())
                .and_then(|o| o.get("hostname"))
                .and_then(|v| v.as_str())
            {
                if !h.is_empty() { c.hostname = h.to_string(); }
            }
            influxdb::write(flat, &c)
        }
        n if n == influxdb2::NAME => {
            let mut c = influxdb2::Config::default();
            hp_into(args, "influxdb2-host", &mut c.host, &mut c.port);
            set_str(args, "influxdb2-org", &mut c.org);
            set_str(args, "influxdb2-bucket", &mut c.bucket);
            set_str(args, "influxdb2-token", &mut c.token);
            set_opt(args, "influxdb2-file", &mut c.file);
            influxdb2::write(flat, &c)
        }
        n if n == influxdb3::NAME => {
            let mut c = influxdb3::Config::default();
            hp_into(args, "influxdb3-host", &mut c.host, &mut c.port);
            set_str(args, "influxdb3-bucket", &mut c.bucket);
            set_str(args, "influxdb3-token", &mut c.token);
            set_opt(args, "influxdb3-file", &mut c.file);
            influxdb3::write(flat, &c)
        }
        n if n == statsd::NAME => {
            let mut c = statsd::Config::default();
            hp_into(args, "statsd-host", &mut c.host, &mut c.port);
            statsd::write(flat, &c)
        }
        n if n == riemann::NAME => {
            let mut c = riemann::Config::default();
            hp_into(args, "riemann-host", &mut c.host, &mut c.port);
            if let Some(Ok(t)) = opt(args, "riemann-timeout").map(|v| v.parse()) { c.timeout_secs = t; }
            riemann::write(flat, &c)
        }
        n if n == kafka::NAME => {
            let mut c = kafka::Config::default();
            hp_into(args, "kafka-bootstrap", &mut c.host, &mut c.port);
            set_str(args, "kafka-topic", &mut c.topic);
            kafka::write(snap, &c)
        }
        n if n == nats::NAME => {
            let mut c = nats::Config::default();
            hp_into(args, "nats-server", &mut c.host, &mut c.port);
            set_str(args, "nats-subject", &mut c.subject_prefix);
            nats::write(flat, &c)
        }
        n if n == mqtt::NAME => {
            let mut c = mqtt::Config::default();
            hp_into(args, "mqtt-server", &mut c.host, &mut c.port);
            set_opt(args, "mqtt-user", &mut c.username);
            set_opt(args, "mqtt-password", &mut c.password);
            set_str(args, "mqtt-topic", &mut c.topic_prefix);
            set_str(args, "mqtt-client-id", &mut c.client_id);
            if let Some(Ok(q)) = opt(args, "mqtt-qos").map(|v| v.parse()) { c.qos = q; }
            mqtt::write(flat, &c)
        }
        n if n == mongodb::NAME => {
            let mut c = mongodb::Config::default();
            hp_into(args, "mongodb-uri", &mut c.host, &mut c.port);
            set_str(args, "mongodb-db", &mut c.database);
            set_str(args, "mongodb-collection", &mut c.collection);
            mongodb::write(flat, &c)
        }
        n if n == cassandra::NAME => {
            let mut c = cassandra::Config::default();
            hp_into(args, "cassandra-host", &mut c.host, &mut c.port);
            set_str(args, "cassandra-keyspace", &mut c.keyspace);
            set_str(args, "cassandra-table", &mut c.table);
            cassandra::write(flat, &c)
        }
        n if n == clickhouse::NAME => {
            let mut c = clickhouse::Config::default();
            hp_into(args, "clickhouse-host", &mut c.host, &mut c.port);
            set_str(args, "clickhouse-db", &mut c.database);
            set_str(args, "clickhouse-table", &mut c.table);
            clickhouse::write(flat, &c)
        }
        n if n == couchdb::NAME => {
            let mut c = couchdb::Config::default();
            hp_into(args, "couchdb-host", &mut c.host, &mut c.port);
            set_str(args, "couchdb-db", &mut c.database);
            set_opt(args, "couchdb-user", &mut c.auth_user);
            set_opt(args, "couchdb-password", &mut c.auth_pass);
            couchdb::write(flat, &c)
        }
        n if n == elasticsearch::NAME => {
            let mut c = elasticsearch::Config::default();
            hp_into(args, "elasticsearch-host", &mut c.host, &mut c.port);
            set_str(args, "elasticsearch-index", &mut c.index);
            set_opt(args, "elasticsearch-user", &mut c.auth_user);
            set_opt(args, "elasticsearch-password", &mut c.auth_pass);
            elasticsearch::write(snap, &c)
        }
        n if n == opentsdb::NAME => {
            let mut c = opentsdb::Config::default();
            hp_into(args, "opentsdb-host", &mut c.host, &mut c.port);
            opentsdb::write(flat, &c)
        }
        n if n == rabbitmq::NAME => {
            let mut c = rabbitmq::Config::default();
            // amqp://[user:pass@]host:port/vhost — strip scheme + userinfo.
            let url = opt(args, "rabbitmq-url").unwrap_or_default();
            let rest = url.strip_prefix("amqp://")
                .or_else(|| url.strip_prefix("amqps://"))
                .unwrap_or(url);
            let rest = rest.rsplit('@').next().unwrap_or(rest);
            let mut seg = rest.splitn(2, '/');
            if let Some(hp_str) = seg.next() {
                if !hp_str.is_empty() {
                    let (h, p) = split_host_port(hp_str, c.port);
                    c.host = h; c.port = p;
                }
            }
            if let Some(vh) = seg.next() {
                if !vh.is_empty() { c.vhost = vh.to_string(); }
            }
            set_str(args, "rabbitmq-exchange", &mut c.exchange);
            set_str(args, "rabbitmq-routing-key", &mut c.routing_key);
            set_str(args, "rabbitmq-user", &mut c.username);
            set_str(args, "rabbitmq-password", &mut c.password);
            rabbitmq::write(flat, &c)
        }
        n if n == restful::NAME => {
            let mut c = restful::Config::default();
            let url = opt(args, "restful-url").unwrap_or_default();
            if url.starts_with("https://") {
                // No TLS in std — silently downgrading to plaintext would
                // leak the snapshot (and any auth token) unencrypted.
                return Err(GlancesError::InvalidConfig(
                    "restful exporter does not support https:// URLs (no TLS); use http://".into(),
                ));
            }
            let rest = url.strip_prefix("http://").unwrap_or(url);
            let mut it = rest.splitn(2, '/');
            if let Some(hp_str) = it.next() {
                if !hp_str.is_empty() {
                    let (h, p) = split_host_port(hp_str, c.port);
                    c.host = h; c.port = p;
                }
            }
            if let Some(p) = it.next() { c.path = format!("/{}", p); }
            set_opt(args, "restful-auth-token", &mut c.auth_token);
            restful::write(snap, &c)
        }
        n if n == prometheus::NAME => {
            let mut c = prometheus::Config::default();
            if let Some(Ok(p)) = opt(args, "prometheus-port").map(|v| v.parse()) { c.port = p; }
            set_str(args, "prometheus-prefix", &mut c.prefix);
            set_opt(args, "prometheus-file", &mut c.file);
            if let Some(l) = opt(args, "prometheus-labels") {
                c.labels = prometheus::parse_labels(&l);
            }
            prometheus::write(flat, &c)
        }
        other => Err(GlancesError::Parse(format!("unknown export target: {}", other))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_host_port_handles_ipv6() {
        assert_eq!(split_host_port("[::1]:8080", 9), ("::1".to_string(), 8080));
        assert_eq!(split_host_port("[::1]", 9), ("::1".to_string(), 9));
        assert_eq!(split_host_port("::1", 9), ("::1".to_string(), 9));
        assert_eq!(split_host_port("h:1", 9), ("h".to_string(), 1));
        assert_eq!(split_host_port("h", 9), ("h".to_string(), 9));
    }

    #[test]
    fn opt_last_wins() {
        let mut a = Args::default();
        a.export_opts.push(("csv-file".into(), "first".into()));
        a.export_opts.push(("csv-file".into(), "last".into()));
        assert_eq!(opt(&a, "csv-file"), Some("last"));
    }
}
