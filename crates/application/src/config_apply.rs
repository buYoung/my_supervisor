//! Pure config-apply planning. Side effects remain in `OperationsFacade`, while
//! this module makes dry-run and apply share exactly the same diff calculation.

use std::collections::{BTreeSet, HashMap};

use my_supervisor_core::domain::{ApplyMode, ConfigDiff, ConfigSnapshot, LoadedConfig, ManagementMode};

pub fn target_snapshot(
    current: &ConfigSnapshot,
    loaded: &LoadedConfig,
    mode: ApplyMode,
) -> ConfigSnapshot {
    let mut processes: HashMap<String, _> = current
        .processes
        .iter()
        .cloned()
        .map(|spec| (spec.name.clone(), spec))
        .collect();
    let mut jobs: HashMap<String, _> = current
        .jobs
        .iter()
        .cloned()
        .map(|job| (job.name.clone(), job))
        .collect();
    if matches!(mode, ApplyMode::Replace) {
        processes.clear();
        jobs.clear();
    }
    for spec in &loaded.processes {
        processes.insert(spec.name.clone(), spec.clone());
    }
    for job in &loaded.jobs {
        let mut job = job.clone();
        if let Some(existing) = current.jobs.iter().find(|existing| existing.name == job.name) {
            job.id = existing.id;
        }
        jobs.insert(job.name.clone(), job);
    }
    let mut processes: Vec<_> = processes.into_values().collect();
    let mut jobs: Vec<_> = jobs.into_values().collect();
    processes.sort_by(|left, right| left.name.cmp(&right.name));
    jobs.sort_by(|left, right| left.name.cmp(&right.name));
    ConfigSnapshot {
        processes,
        jobs,
        running_direct_processes: current.running_direct_processes.clone(),
    }
}

pub fn diff(current: &ConfigSnapshot, target: &ConfigSnapshot) -> ConfigDiff {
    let current_processes: HashMap<_, _> = current.processes.iter().map(|spec| (&spec.name, spec)).collect();
    let target_processes: HashMap<_, _> = target.processes.iter().map(|spec| (&spec.name, spec)).collect();
    let current_jobs: HashMap<_, _> = current.jobs.iter().map(|job| (&job.name, job)).collect();
    let target_jobs: HashMap<_, _> = target.jobs.iter().map(|job| (&job.name, job)).collect();
    ConfigDiff {
        added_processes: names_only(&target_processes, &current_processes),
        updated_processes: changed_names(&target_processes, &current_processes),
        removed_processes: names_only(&current_processes, &target_processes),
        added_jobs: names_only(&target_jobs, &current_jobs),
        updated_jobs: changed_names(&target_jobs, &current_jobs),
        removed_jobs: names_only(&current_jobs, &target_jobs),
    }
}

/// Direct processes that must be running once a config saga reaches its target.
/// A previous runtime is an operational fact, not an `autostart` preference:
/// when the same Direct process remains in the target it must restart with the
/// target spec even if that target disables autostart.
pub fn desired_running_target(previous: &ConfigSnapshot, target: &ConfigSnapshot) -> Vec<String> {
    let previously_running: BTreeSet<_> = previous.running_direct_processes.iter().collect();
    target
        .processes
        .iter()
        .filter(|spec| {
            matches!(spec.management_mode, ManagementMode::Direct)
                && (spec.autostart || previously_running.contains(&spec.name))
        })
        .map(|spec| spec.name.clone())
        .collect()
}

fn names_only<'a, T>(left: &HashMap<&'a String, T>, right: &HashMap<&'a String, T>) -> Vec<String> {
    let names: BTreeSet<String> = left.keys().filter(|name| !right.contains_key(*name)).map(|name| (*name).clone()).collect();
    names.iter().cloned().collect()
}

fn changed_names<'a, T: PartialEq>(left: &HashMap<&'a String, T>, right: &HashMap<&'a String, T>) -> Vec<String> {
    let names: BTreeSet<String> = left.iter().filter_map(|(name, value)| right.get(*name).filter(|other| *other != value).map(|_| (*name).clone())).collect();
    names.iter().cloned().collect()
}
