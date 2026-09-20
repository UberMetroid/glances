//! Per-tick alert-command dispatch (upstream get_limit_action + run).

use super::actions::GlancesActions;
use super::plugin::Plugin;
use super::value::Value;

/// Fire configured `*_action` commands for a plugin's live
/// CAREFUL/WARNING/CRITICAL triggers (upstream `get_limit_action` +
/// `GlancesActions.run` parity). Mustache dict = top-level scalar stats.
pub(crate) fn run_plugin_actions(plugin: &mut dyn Plugin, actions: &mut GlancesActions) {
    let Some(model) = plugin.model_mut() else { return };
    let triggers: Vec<(String, String)> = model
        .thresholds
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let dict: std::collections::BTreeMap<String, String> = match &model.stats {
        Value::Object(o) => o
            .iter()
            .filter_map(|(k, v)| {
                let s = match v {
                    Value::String(s) => s.clone(),
                    Value::Int(i) => i.to_string(),
                    Value::Uint(u) => u.to_string(),
                    Value::Float(f) => format!("{}", f),
                    Value::Bool(b) => b.to_string(),
                    _ => return None,
                };
                Some((k.clone(), s))
            })
            .collect(),
        _ => std::collections::BTreeMap::new(),
    };
    for (stat, trigger) in &triggers {
        if !matches!(trigger.as_str(), "CAREFUL" | "WARNING" | "CRITICAL") {
            continue;
        }
        let (cmds, repeat) = model.get_limit_action(&trigger.to_lowercase(), stat);
        if let Some(cmds) = cmds {
            actions.run(stat, trigger, &cmds, repeat, &dict);
        }
    }
}
