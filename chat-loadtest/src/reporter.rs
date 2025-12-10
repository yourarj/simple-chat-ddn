use crate::metrics::LoadTestMetrics;
use anyhow::Result;
use colored::Colorize;
use std::fs::File;
use std::io::Write;

pub fn print_text(metrics: &LoadTestMetrics, output: Option<String>) -> Result<()> {
  let report = format_text_report(metrics);

  match output {
    Some(path) => {
      let mut file = File::create(path)?;
      file.write_all(report.as_bytes())?;
    }
    None => {
      println!("{}", report);
    }
  }

  Ok(())
}

fn format_text_report(metrics: &LoadTestMetrics) -> String {
  let mut report = String::new();

  report.push_str(&format!(
    "\n{}\n",
    "╔═══════════════════════════════════════════════════════════╗".cyan()
  ));
  report.push_str(&format!(
    "{}\n",
    "║                    Load Test Results                     ║".cyan()
  ));
  report.push_str(&format!(
    "{}\n\n",
    "╚═══════════════════════════════════════════════════════════╝".cyan()
  ));

  // Duration
  report.push_str(&format!(
    "{}  {:.2}s\n",
    "Duration:".bold(),
    metrics.duration.as_secs_f64()
  ));
  report.push('\n');

  // Connections
  report.push_str(&format!("{}\n", "Connections:".bold().underline()));
  report.push_str(&format!(
    "  Attempted:   {}\n",
    metrics.connections_attempted
  ));
  report.push_str(&format!(
    "  {}  {}\n",
    "Successful:".green(),
    metrics.connections_successful
  ));
  report.push_str(&format!(
    "  {}    {}\n",
    "Failed:".red(),
    metrics.connections_failed
  ));

  let success_rate = if metrics.connections_attempted > 0 {
    (metrics.connections_successful as f64 / metrics.connections_attempted as f64) * 100.0
  } else {
    0.0
  };
  report.push_str(&format!("  Success Rate: {:.2}%\n", success_rate));
  report.push('\n');

  // Messages
  report.push_str(&format!("{}\n", "Messages:".bold().underline()));
  report.push_str(&format!("  Sent:        {}\n", metrics.messages_sent));
  report.push_str(&format!("  Received:    {}\n", metrics.messages_received));
  report.push_str(&format!(
    "  {}      {}\n",
    "Failed:".red(),
    metrics.messages_failed
  ));
  report.push_str(&format!(
    "  Throughput:  {:.2} msg/s\n",
    metrics.messages_per_second
  ));
  report.push('\n');

  // Bandwidth
  report.push_str(&format!("{}\n", "Bandwidth:".bold().underline()));
  report.push_str(&format!(
    "  Sent:        {} KB ({:.2} KB/s)\n",
    metrics.bytes_sent / 1024,
    (metrics.bytes_sent as f64 / 1024.0) / metrics.duration.as_secs_f64()
  ));
  report.push_str(&format!(
    "  Received:    {} KB ({:.2} KB/s)\n",
    metrics.bytes_received / 1024,
    (metrics.bytes_received as f64 / 1024.0) / metrics.duration.as_secs_f64()
  ));
  report.push('\n');

  // Latency
  if !metrics.latency_histogram.is_empty() {
    report.push_str(&format!("{}\n", "Latency:".bold().underline()));
    report.push_str(&format!("  Min:         {:?}\n", metrics.get_min_latency()));
    report.push_str(&format!(
      "  Mean:        {:?}\n",
      metrics.get_mean_latency()
    ));
    report.push_str(&format!("  Max:         {:?}\n", metrics.get_max_latency()));
    report.push_str(&format!(
      "  Std Dev:     {:?}\n",
      metrics.get_stddev_latency()
    ));
    report.push('\n');

    // Percentiles
    report.push_str(&format!("{}\n", "Percentiles:".bold().underline()));
    for &p in &[50.0, 90.0, 95.0, 99.0, 99.9] {
      report.push_str(&format!(
        "  p{:<5}      {:?}\n",
        p,
        metrics.get_percentile(p)
      ));
    }
  }

  // Errors
  if !metrics.errors.is_empty() {
    report.push('\n');
    report.push_str(&format!("{}\n", "Errors:".bold().underline().red()));
    report.push_str(&format!("  Total:       {}\n", metrics.errors.len()));

    // Error summary by type
    let error_summary = metrics.get_error_summary();
    for (error_type, count) in error_summary {
      report.push_str(&format!("  {:?}: {}\n", error_type, count));
    }

    // Show first 10 errors
    if !metrics.errors.is_empty() {
      report.push_str("\n  First errors:\n");
      for (i, error) in metrics.errors.iter().take(10).enumerate() {
        report.push_str(&format!(
          "    {}. [{:?}] {}: {} - {}\n",
          i + 1,
          error.timestamp,
          error.user,
          format!("{:?}", error.error_type).red(),
          error.message
        ));
      }

      if metrics.errors.len() > 10 {
        report.push_str(&format!("    ... and {} more\n", metrics.errors.len() - 10));
      }
    }
  }

  report
}

pub fn print_json(metrics: &LoadTestMetrics, output: Option<String>) -> Result<()> {
  let errors_json: Vec<_> = metrics
    .errors
    .iter()
    .map(|e| {
      serde_json::json!({
          "timestamp_secs": e.timestamp.as_secs_f64(),
          "user": e.user,
          "type": format!("{:?}", e.error_type),
          "message": e.message,
      })
    })
    .collect();

  let json = serde_json::json!({
      "duration_secs": metrics.duration.as_secs_f64(),
      "connections": {
          "attempted": metrics.connections_attempted,
          "successful": metrics.connections_successful,
          "failed": metrics.connections_failed,
      },
      "messages": {
          "sent": metrics.messages_sent,
          "received": metrics.messages_received,
          "failed": metrics.messages_failed,
          "throughput": metrics.messages_per_second,
      },
      "bandwidth": {
          "bytes_sent": metrics.bytes_sent,
          "bytes_received": metrics.bytes_received,
      },
      "latency": if !metrics.latency_histogram.is_empty() {
          serde_json::json!({
              "min_us": metrics.get_min_latency().as_micros(),
              "mean_us": metrics.get_mean_latency().as_micros(),
              "max_us": metrics.get_max_latency().as_micros(),
              "stddev_us": metrics.get_stddev_latency().as_micros(),
              "p50_us": metrics.get_percentile(50.0).as_micros(),
              "p90_us": metrics.get_percentile(90.0).as_micros(),
              "p95_us": metrics.get_percentile(95.0).as_micros(),
              "p99_us": metrics.get_percentile(99.0).as_micros(),
              "p99_9_us": metrics.get_percentile(99.9).as_micros(),
          })
      } else {
          serde_json::json!(null)
      },
      "errors": {
          "count": metrics.errors.len(),
          "details": errors_json,
      },
  });

  let json_str = serde_json::to_string_pretty(&json)?;

  match output {
    Some(path) => {
      let mut file = File::create(path)?;
      file.write_all(json_str.as_bytes())?;
    }
    None => {
      println!("{}", json_str);
    }
  }

  Ok(())
}

pub fn print_csv(metrics: &LoadTestMetrics, output: Option<String>) -> Result<()> {
  let mut csv = String::new();
  csv.push_str("metric,value\n");
  csv.push_str(&format!(
    "duration_secs,{}\n",
    metrics.duration.as_secs_f64()
  ));
  csv.push_str(&format!(
    "connections_attempted,{}\n",
    metrics.connections_attempted
  ));
  csv.push_str(&format!(
    "connections_successful,{}\n",
    metrics.connections_successful
  ));
  csv.push_str(&format!(
    "connections_failed,{}\n",
    metrics.connections_failed
  ));
  csv.push_str(&format!("messages_sent,{}\n", metrics.messages_sent));
  csv.push_str(&format!(
    "messages_received,{}\n",
    metrics.messages_received
  ));
  csv.push_str(&format!("messages_failed,{}\n", metrics.messages_failed));
  csv.push_str(&format!(
    "throughput_msg_per_sec,{}\n",
    metrics.messages_per_second
  ));
  csv.push_str(&format!("bytes_sent,{}\n", metrics.bytes_sent));
  csv.push_str(&format!("bytes_received,{}\n", metrics.bytes_received));
  csv.push_str(&format!("errors_count,{}\n", metrics.errors.len()));

  if !metrics.latency_histogram.is_empty() {
    csv.push_str(&format!(
      "latency_min_us,{}\n",
      metrics.get_min_latency().as_micros()
    ));
    csv.push_str(&format!(
      "latency_mean_us,{}\n",
      metrics.get_mean_latency().as_micros()
    ));
    csv.push_str(&format!(
      "latency_max_us,{}\n",
      metrics.get_max_latency().as_micros()
    ));
    csv.push_str(&format!(
      "latency_p50_us,{}\n",
      metrics.get_percentile(50.0).as_micros()
    ));
    csv.push_str(&format!(
      "latency_p90_us,{}\n",
      metrics.get_percentile(90.0).as_micros()
    ));
    csv.push_str(&format!(
      "latency_p95_us,{}\n",
      metrics.get_percentile(95.0).as_micros()
    ));
    csv.push_str(&format!(
      "latency_p99_us,{}\n",
      metrics.get_percentile(99.0).as_micros()
    ));
    csv.push_str(&format!(
      "latency_p99_9_us,{}\n",
      metrics.get_percentile(99.9).as_micros()
    ));
  }

  match output {
    Some(path) => {
      let mut file = File::create(path)?;
      file.write_all(csv.as_bytes())?;
    }
    None => {
      println!("{}", csv);
    }
  }

  Ok(())
}
