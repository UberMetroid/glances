//! Tokenizer for argv.
//!
//! Splits raw argv strings into typed `Token`s (`Flag`, `WithValue`, `Positional`).
//! The dispatch in `flags.rs` decides what each token does.

/// A single argv token after minimal parsing.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Flag(String),
    WithValue { name: String, value: String },
    Positional(String),
}

/// Tokenize a flat list of argv strings. No globbing, no expansion — strictly
/// shell-quoting-free positional split.
pub fn parse_argv(argv: &[String]) -> Vec<Token> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < argv.len() {
        let arg = &argv[i];
        // Literal "--" ends option processing (argparse semantics):
        // everything after it is positional.
        if arg == "--" {
            for rest in &argv[i + 1..] {
                out.push(Token::Positional(rest.clone()));
            }
            break;
        }
        if let Some(eq_pos) = arg.find('=') {
            // --flag=value or -x=value form
            if arg.starts_with("--") || arg.starts_with('-') {
                let (name, value) = arg.split_at(eq_pos);
                out.push(Token::WithValue {
                    name: name.to_string(),
                    value: value[1..].to_string(),
                });
            } else {
                out.push(Token::Positional(arg.clone()));
            }
            i += 1;
            continue;
        }
        if arg.starts_with("--") {
            let name = arg.clone();
            // Long flag may take a value (next argv if non-negative).
            if i + 1 < argv.len() && !argv[i + 1].starts_with('-') && looks_like_value_for(&name) {
                out.push(Token::WithValue { name, value: argv[i + 1].clone() });
                i += 2;
            } else {
                out.push(Token::Flag(name));
                i += 1;
            }
        } else if arg.starts_with('-') && arg.len() > 1 {
            // Short flag cluster. Walk chars left-to-right (argparse
            // semantics): a value-taking short option consumes the REST
            // of the token as its value (`-t5`, `-clocalhost`), or the
            // next argv element when it's the last char (`-st 5`).
            // Reinterpreting a value's chars as flags is how `-clocalhost`
            // used to launch an unauthenticated XML-RPC server.
            let chars: Vec<char> = arg.chars().skip(1).collect();
            let mut j = 0;
            let mut consumed_next = false;
            while j < chars.len() {
                let name = format!("-{}", chars[j]);
                if looks_like_value_for(&name) {
                    let rest: String = chars[j + 1..].iter().collect();
                    if !rest.is_empty() {
                        out.push(Token::WithValue { name, value: rest });
                    } else if i + 1 < argv.len() && !argv[i + 1].starts_with('-') {
                        out.push(Token::WithValue { name, value: argv[i + 1].clone() });
                        consumed_next = true;
                    } else {
                        out.push(Token::Flag(name));
                    }
                    break;
                }
                out.push(Token::Flag(name));
                j += 1;
            }
            i += if consumed_next { 2 } else { 1 };
        } else {
            out.push(Token::Positional(arg.clone()));
            i += 1;
        }
    }
    out
}

/// Flags that always take a following value.
fn looks_like_value_for(flag: &str) -> bool {
    matches!(flag,
        "-t" | "--time"
        | "-c" | "--client"
        | "-B" | "--bind"
        | "-u" | "--username"
        | "-C" | "--config"
        | "-P" | "--plugins"
        | "-f" | "--process-filter"
        | "--sort-processes"
        | "--process-focus"
        | "--strftime"
        | "--fetch-template"
        | "--stdout-fetch-template"
        | "--snmp-auth"
        | "--snmp-user"
        | "--stdout-csv"
        | "--stdout-json"
        | "--stop-after"
        | "--url-prefix"
        | "--cached-time"
        | "--snmp-community"
        | "--snmp-port"
        | "--snmp-version"
        | "--password"
        | "--mcp-path"
        | "--secure-config"
        | "--stdout"
        | "--web-port"
        | "--disable-plugin"
        | "--enable-plugin"
    )
}
