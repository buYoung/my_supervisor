use std::path::PathBuf;

use chrono::{DateTime, Utc};
use clap::Args;
use comfy_table::Table;
use my_supervisor_shared::logs::{LogQueryDto, LogQueryResultDto};
use tokio::io::AsyncWriteExt;

use crate::client::{CliError, Client};

#[derive(Debug, Args)]
pub struct LogSearchArgs {
    #[arg(long, default_value = "process", value_parser = ["process", "job"])]
    kind: String,
    #[arg(long)]
    source: Option<String>,
    #[arg(long, requires = "source")]
    run_id: Option<uuid::Uuid>,
    /// attention, all, without_info, or comma-separated trace/debug/info/warn/error/fatal/unknown.
    #[arg(long, default_value = "attention")]
    level: String,
    #[arg(long, value_parser = ["stdout", "stderr", "system"])]
    stream: Option<String>,
    #[arg(long)]
    search: Option<String>,
    /// Exact decoded message, useful when expanding one repeat group.
    #[arg(long)]
    message: Option<String>,
    #[arg(long)]
    instance: Option<String>,
    #[arg(long)]
    step: Option<String>,
    #[arg(long)]
    since: Option<DateTime<Utc>>,
    #[arg(long)]
    until: Option<DateTime<Utc>>,
    #[arg(long, default_value = "repeat", value_parser = ["repeat", "none"])]
    grouping: String,
    #[arg(long)]
    ascending: bool,
    #[arg(long, default_value_t = 0)]
    offset: usize,
    #[arg(long, default_value_t = 50)]
    limit: usize,
    /// Export matching individual records as JSONL, retaining original text.
    #[arg(long)]
    export: Option<PathBuf>,
    /// Refresh the filtered/grouped snapshot every five seconds. Ctrl-C exits.
    #[arg(short = 'f', long, conflicts_with_all = ["export", "until", "offset"])]
    follow: bool,
}

pub async fn execute(client: &Client, args: &LogSearchArgs, json: bool) -> Result<(), CliError> {
    let mut query = LogQueryDto {
        kind: args.kind.clone(),
        source: args.source.clone(),
        run_id: args.run_id,
        level: args.level.clone(),
        stream: args.stream.clone(),
        search: args.search.clone(),
        message: args.message.clone(),
        instance: args.instance.clone(),
        step: args.step.clone(),
        since: args.since,
        until: args.until,
        grouping: args.grouping.clone(),
        ascending: args.ascending,
        offset: args.offset,
        limit: args.limit,
    };
    if let Some(path) = &args.export {
        // create_new avoids silently replacing an existing operator export.
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .await
            .map_err(|error| CliError::Failed(format!("{}: {error}", path.display())))?;
        query.grouping = "none".into();
        query.limit = 5_000;
        query.offset = 0;
        let mut count = 0;
        loop {
            let page = client.query_logs(&query).await?;
            query.until = Some(page.snapshot_at);
            if !page.partial_sources.is_empty() {
                eprintln!("partial sources: {}", page.partial_sources.join(", "));
            }
            for group in page.records {
                let mut line = serde_json::to_vec(&group.entry)
                    .map_err(|error| CliError::Failed(error.to_string()))?;
                line.push(b'\n');
                file.write_all(&line)
                    .await
                    .map_err(|error| CliError::Failed(error.to_string()))?;
                count += 1;
            }
            match page.next_offset {
                Some(offset) => query.offset = offset,
                None => break,
            }
        }
        file.flush()
            .await
            .map_err(|error| CliError::Failed(error.to_string()))?;
        println!("exported {count} records to {}", path.display());
        return Ok(());
    }
    if args.follow {
        let mut previous = String::new();
        loop {
            let result = tokio::select! {
                result = client.query_logs(&query) => result?,
                _ = tokio::signal::ctrl_c() => return Ok(()),
            };
            let signature = format!(
                "{:?}:{:?}:{:?}",
                result
                    .records
                    .iter()
                    .map(|record| (&record.entry.id, record.count))
                    .collect::<Vec<_>>(),
                result.total_lines,
                result.partial_sources
            );
            if signature != previous {
                if json && crate::JSON_V2_OUTPUT.load(std::sync::atomic::Ordering::Relaxed) {
                    crate::print_json(&result)?;
                } else if json {
                    println!(
                        "{}",
                        serde_json::to_string(&result)
                            .map_err(|error| CliError::Failed(error.to_string()))?
                    );
                } else {
                    println!("\nSnapshot: {}", result.snapshot_at.to_rfc3339());
                    print_result(&result, false)?;
                }
                previous = signature;
            }
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {},
                _ = tokio::signal::ctrl_c() => return Ok(()),
            }
        }
    }
    print_result(&client.query_logs(&query).await?, json)
}

fn print_result(result: &LogQueryResultDto, json: bool) -> Result<(), CliError> {
    if json {
        return crate::print_json(result);
    }
    let mut table = Table::new();
    table.set_header([
        "LEVEL",
        "SOURCE / RUN",
        "COUNT",
        "FIRST",
        "LATEST",
        "MESSAGE",
    ]);
    for group in &result.records {
        let source = group.entry.run_id.map_or_else(
            || group.entry.source.clone(),
            |id| format!("{} / {id}", group.entry.source),
        );
        table.add_row([
            group.entry.level.to_uppercase(),
            source,
            group.count.to_string(),
            group.first_seen.to_rfc3339(),
            group.last_seen.to_rfc3339(),
            group.entry.message.clone(),
        ]);
    }
    println!("{table}");
    println!(
        "{} records · {} groups · {} scanned",
        result.total_lines, result.total_groups, result.scanned_lines
    );
    if let Some(offset) = result.next_offset {
        println!(
            "next: --offset {offset} --until {}",
            result.snapshot_at.to_rfc3339()
        );
    }
    if !result.partial_sources.is_empty() {
        eprintln!("partial sources: {}", result.partial_sources.join(", "));
    }
    Ok(())
}
