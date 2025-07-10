/* Licensed to the Apache Software Foundation (ASF) under one
 * or more contributor license agreements.  See the NOTICE file
 * distributed with this work for additional information
 * regarding copyright ownership.  The ASF licenses this file
 * to you under the Apache License, Version 2.0 (the
 * "License"); you may not use this file except in compliance
 * with the License.  You may obtain a copy of the License at
 *
 *   http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing,
 * software distributed under the License is distributed on an
 * "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
 * KIND, either express or implied.  See the License for the
 * specific language governing permissions and limitations
 * under the License.
 */

#![allow(clippy::cast_precision_loss)]
#![allow(clippy::struct_field_names)]

use crate::analytics::time_series::{
    calculator::TimeSeriesCalculator,
    processors::{TimeSeriesProcessor, moving_average::MovingAverageProcessor},
};
use crate::info;
use bench_report::{
    actor_kind::ActorKind,
    group_metrics::BenchmarkGroupMetrics,
    group_metrics_kind::GroupMetricsKind,
    group_metrics_summary::BenchmarkGroupMetricsSummary,
    individual_metrics::BenchmarkIndividualMetrics,
    utils::{max, min, std_dev},
};

pub fn from_producers_and_consumers_statistics(
    producers_stats: &[BenchmarkIndividualMetrics],
    consumers_stats: &[BenchmarkIndividualMetrics],
    moving_average_window: u32,
) -> Option<BenchmarkGroupMetrics> {
    let mut summary = from_individual_metrics(
        &[producers_stats, consumers_stats].concat(),
        moving_average_window,
    )?;
    summary.summary.kind = GroupMetricsKind::ProducersAndConsumers;
    Some(summary)
}

pub fn from_individual_metrics(
    stats: &[BenchmarkIndividualMetrics],
    moving_average_window: u32,
) -> Option<BenchmarkGroupMetrics> {
    if stats.is_empty() {
        return None;
    }
    let now = std::time::SystemTime::now();

    let kind = if stats.len() == 200 {
        determine_group_kind(stats)
    } else {
        GroupMetricsKind::ProducersAndConsumers
    };
    info!("{kind}: len {}", stats.len());
    log_event_at_time_with_kind("Completed determine_group_kind", now, kind);
    let throughput_metrics = calculate_throughput_metrics(stats);
    log_event_at_time_with_kind("Completed calculate_throughput_metrics", now, kind);
    let latency_metrics = calculate_latency_metrics(stats);
    log_event_at_time_with_kind("Completed calculate_latency_metrics", now, kind);
    let time_series = calculate_group_time_series(stats, moving_average_window);
    log_event_at_time_with_kind("Completed calculate_group_time_series", now, kind);
    let (min_latency_ms_value, max_latency_ms_value) =
        calculate_min_max_latencies(stats, &time_series.2);
    log_event_at_time_with_kind("Completed calculate_min_max_latencies", now, kind);

    let summary = BenchmarkGroupMetricsSummary {
        kind,
        total_throughput_megabytes_per_second: throughput_metrics.total_megabytes_per_sec,
        total_throughput_messages_per_second: throughput_metrics.total_messages_per_sec,
        average_throughput_megabytes_per_second: throughput_metrics.average_megabytes_per_sec,
        average_throughput_messages_per_second: throughput_metrics.average_messages_per_sec,
        average_p50_latency_ms: latency_metrics.p50_latency,
        average_p90_latency_ms: latency_metrics.p90_latency,
        average_p95_latency_ms: latency_metrics.p95_latency,
        average_p99_latency_ms: latency_metrics.p99_latency,
        average_p999_latency_ms: latency_metrics.p999_latency,
        average_p9999_latency_ms: latency_metrics.p9999_latency,
        average_latency_ms: latency_metrics.average_latency,
        average_median_latency_ms: latency_metrics.median_latency,
        min_latency_ms: min_latency_ms_value,
        max_latency_ms: max_latency_ms_value,
        std_dev_latency_ms: std_dev(&time_series.2).unwrap_or(0.0),
    };

    log_event_at_time_with_kind("Completed from_individual_metrics", now, kind);
    Some(BenchmarkGroupMetrics {
        summary,
        avg_throughput_mb_ts: time_series.0,
        avg_throughput_msg_ts: time_series.1,
        avg_latency_ts: time_series.2,
    })
}

struct ThroughputMetrics {
    total_megabytes_per_sec: f64,
    total_messages_per_sec: f64,
    average_megabytes_per_sec: f64,
    average_messages_per_sec: f64,
}

fn calculate_throughput_metrics(stats: &[BenchmarkIndividualMetrics]) -> ThroughputMetrics {
    let count = stats.len() as f64;
    let total_mb_per_sec = stats
        .iter()
        .map(|r| r.summary.throughput_megabytes_per_second)
        .sum();
    let total_msg_per_sec = stats
        .iter()
        .map(|r| r.summary.throughput_messages_per_second)
        .sum();

    ThroughputMetrics {
        total_megabytes_per_sec: total_mb_per_sec,
        total_messages_per_sec: total_msg_per_sec,
        average_megabytes_per_sec: total_mb_per_sec / count,
        average_messages_per_sec: total_msg_per_sec / count,
    }
}

struct LatencyMetrics {
    p50_latency: f64,
    p90_latency: f64,
    p95_latency: f64,
    p99_latency: f64,
    p999_latency: f64,
    p9999_latency: f64,
    average_latency: f64,
    median_latency: f64,
}

fn calculate_latency_metrics(stats: &[BenchmarkIndividualMetrics]) -> LatencyMetrics {
    let count = stats.len() as f64;

    LatencyMetrics {
        p50_latency: stats.iter().map(|r| r.summary.p50_latency_ms).sum::<f64>() / count,
        p90_latency: stats.iter().map(|r| r.summary.p90_latency_ms).sum::<f64>() / count,
        p95_latency: stats.iter().map(|r| r.summary.p95_latency_ms).sum::<f64>() / count,
        p99_latency: stats.iter().map(|r| r.summary.p99_latency_ms).sum::<f64>() / count,
        p999_latency: stats.iter().map(|r| r.summary.p999_latency_ms).sum::<f64>() / count,
        p9999_latency: stats
            .iter()
            .map(|r| r.summary.p9999_latency_ms)
            .sum::<f64>()
            / count,
        average_latency: stats.iter().map(|r| r.summary.avg_latency_ms).sum::<f64>() / count,
        median_latency: stats
            .iter()
            .map(|r| r.summary.median_latency_ms)
            .sum::<f64>()
            / count,
    }
}

fn determine_group_kind(stats: &[BenchmarkIndividualMetrics]) -> GroupMetricsKind {
    match stats.iter().next().unwrap().summary.actor_kind {
        ActorKind::Producer => GroupMetricsKind::Producers,
        ActorKind::Consumer => GroupMetricsKind::Consumers,
        ActorKind::ProducingConsumer => GroupMetricsKind::ProducingConsumers,
    }
}

use bench_report::time_series::TimeSeries;

fn calculate_group_time_series(
    stats: &[BenchmarkIndividualMetrics],
    moving_average_window: u32,
) -> (TimeSeries, TimeSeries, TimeSeries) {
    let mut now = std::time::SystemTime::now();
    log_and_update_duration("Begin time series ops", &mut now);

    let mut avg_throughput_mb_ts = TimeSeriesCalculator::aggregate_sum(
        &stats
            .iter()
            .map(|r| r.throughput_mb_ts.clone())
            .collect::<Vec<_>>(),
    );
    log_and_update_duration("Extract MB time series", &mut now);

    let mut avg_throughput_msg_ts = TimeSeriesCalculator::aggregate_sum(
        &stats
            .iter()
            .map(|r| r.throughput_msg_ts.clone())
            .collect::<Vec<_>>(),
    );
    log_and_update_duration("Extract message time series", &mut now);

    let mut avg_latency_ts = TimeSeriesCalculator::aggregate_avg(
        &stats
            .iter()
            .map(|r| r.latency_ts.clone())
            .collect::<Vec<_>>(),
    );
    log_and_update_duration("Extract latency time series", &mut now);

    let sma = MovingAverageProcessor::new(moving_average_window as usize);
    avg_throughput_mb_ts = sma.process(&avg_throughput_mb_ts);
    log_and_update_duration("Compute MB moving average", &mut now);
    avg_throughput_msg_ts = sma.process(&avg_throughput_msg_ts);
    log_and_update_duration("Compute message moving average", &mut now);
    avg_latency_ts = sma.process(&avg_latency_ts);
    log_and_update_duration("Compute latency moving average", &mut now);

    (avg_throughput_mb_ts, avg_throughput_msg_ts, avg_latency_ts)
}

fn calculate_min_max_latencies(
    stats: &[BenchmarkIndividualMetrics],
    avg_latency_ts: &TimeSeries,
) -> (f64, f64) {
    let min_latency_ms = if stats.is_empty() {
        None
    } else {
        stats
            .iter()
            .map(|s| s.summary.min_latency_ms)
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
    };

    let max_latency_ms = if stats.is_empty() {
        None
    } else {
        stats
            .iter()
            .map(|s| s.summary.max_latency_ms)
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
    };

    let min_latency_ms_value = min_latency_ms.unwrap_or_else(|| min(avg_latency_ts).unwrap_or(0.0));
    let max_latency_ms_value = max_latency_ms.unwrap_or_else(|| max(avg_latency_ts).unwrap_or(0.0));

    (min_latency_ms_value, max_latency_ms_value)
}


fn log_event_at_time_with_kind(event: &str, time: std::time::SystemTime, kind: GroupMetricsKind) {
    match time.elapsed() {
        Ok(duration) => info!("{}: {} @ {} microsec", kind, event, duration.as_micros()),
        Err(_) => info!("{}: {} @ unknown time", kind, event),
    }
}

fn log_and_update_duration(task: &str, now: &mut std::time::SystemTime) {
    match now.elapsed() {
        Ok(duration) => info!("{} took {} microsec", task, duration.as_micros()),
        Err(_) => info!("{} took unknown time", task),
    }
    *now = std::time::SystemTime::now();
}
