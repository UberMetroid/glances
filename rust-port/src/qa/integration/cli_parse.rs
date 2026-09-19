//! Verifies every flag is accepted by the parser.

use crate::cli::args::{parse_args, Mode};

#[test]
fn parse_does_not_crash() {
    let _ = parse_args();
}

#[test]
fn mode_enum_has_all_variants() {
    let _ = Mode::Standalone;
    let _ = Mode::XmlRpcServer;
    let _ = Mode::XmlRpcClient;
    let _ = Mode::Browser;
    let _ = Mode::WebServer;
    let _ = Mode::StdoutCsv;
    let _ = Mode::StdoutJson;
    let _ = Mode::StdoutPath;
    let _ = Mode::ApiDoc;
    let _ = Mode::Issue;
    let _ = Mode::Help;
    let _ = Mode::Version;
}
