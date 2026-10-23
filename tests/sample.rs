use std::future;
use std::sync::{Mutex, OnceLock};

use chrono::{DateTime, Local, NaiveDateTime, NaiveTime, Utc};
use futures::Future;

use watchgha::bucketer::Bucketer;
use watchgha::data::{Fetcher, get_events};
use watchgha::draw::{Renderer, draw_events};
use watchgha::utils::Result;

struct Fake;

impl Fetcher for Fake {
    fn fetch<'a>(&'a self, url: &'a str) -> impl Future<Output = Result<String>> + Send + 'a {
        future::ready(Ok(data(url.strip_suffix("?per_page=100").unwrap_or(url))))
    }
}

fn now_utc() -> DateTime<Utc> {
    let now = Utc::now();
    NaiveDateTime::new(
        now.date_naive(),
        NaiveTime::from_hms_opt(16, 55, 30).unwrap(),
    )
    .and_utc()
}

fn data(url: &str) -> String {
    let now = now_utc();
    let runs = [
        run(
            0,
            now,
            1,
            "Test suite",
            "in_progress",
            "",
            "demo:jobs_tests",
        ),
        run(
            2,
            now,
            3,
            "A success run",
            "completed",
            "success",
            "demo:jobs_1",
        ),
        run(
            4,
            now,
            5,
            "A startup_failure run",
            "completed",
            "startup_failure",
            "demo:jobs_1",
        ),
        run(
            6,
            now,
            7,
            "A skipped run",
            "completed",
            "skipped",
            "demo:jobs_1",
        ),
        run(
            8,
            now,
            9,
            "A failure run",
            "completed",
            "failure",
            "demo:jobs_1",
        ),
        run(
            10,
            now,
            11,
            "A cancelled run",
            "completed",
            "cancelled",
            "demo:jobs_1",
        ),
        run(
            12,
            now,
            13,
            "Malicious \n\n\n\n data",
            "in_progress",
            "",
            "demo:jobs_malicious",
        ),
    ];
    let one = format!("{{\"workflow_runs\":[{}]}}", runs.join(","));
    let jobs_1 = jobs(vec![job(14, now, 15, "Just one job", "queued", "", &[])]);
    let jobs_tests = jobs(vec![
        job(16, now, 17, "A delayed job", "queued", "", &[]),
        job(
            18,
            now,
            19,
            "A failed job",
            "completed",
            "failure",
            &["ok", "bad", "queued"],
        ),
        job(20, now, 21, "A finished job", "completed", "success", &[]),
        py_job(22, now, 23, 8),
        py_job(24, now, 25, 9),
        py_job(26, now, 27, 10),
        py_job(28, now, 29, 11),
    ]);
    let malicious = jobs(vec![job(
        30,
        now,
        31,
        "A \u{1b}[1mMALICIOUS\u{1b}[0m job",
        "queued",
        "",
        &[],
    )]);
    match url {
        "demo:one" => one,
        "demo:jobs_1" => jobs_1,
        "demo:jobs_tests" => jobs_tests,
        "demo:jobs_malicious" => malicious,
        other => panic!("unexpected url {other:?}"),
    }
}

fn run(
    id: u64,
    now: DateTime<Utc>,
    k: u64,
    name: &str,
    status: &str,
    conclusion: &str,
    jobs_url: &str,
) -> String {
    format!(
        "{{\"id\":{id},\"display_title\":\"fix: most awesome fix\",\"head_branch\":\"nedbat/test\",\
         \"html_url\":\"https://github.com/owner/repo/actions/runs/123456789\",\"event\":\"push\",\
         \"head_sha\":\"4b2ff58124791953563fdb52e40d9ab79d274d9a\",\
         \"run_started_at\":\"{}\",\"name\":{},\"status\":\"{status}\",\"conclusion\":\"{conclusion}\",\
         \"jobs_url\":\"{jobs_url}\"}}",
        iso(now, k),
        serde_json::to_string(name).unwrap()
    )
}

fn job(
    id: u64,
    now: DateTime<Utc>,
    k: u64,
    name: &str,
    status: &str,
    conclusion: &str,
    steps: &[&str],
) -> String {
    let steps = steps
        .iter()
        .map(|step| match *step {
            "ok" => "{\"status\":\"completed\",\"conclusion\":\"success\"}",
            "bad" => {
                "{\"status\":\"completed\",\"conclusion\":\"failure\",\"name\":\"Didn't work\"}"
            }
            "queued" => "{\"status\":\"queued\"}",
            _ => unreachable!(),
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"id\":{id},\"created_at\":\"{}\",\"name\":{},\"status\":\"{status}\",\
         \"conclusion\":\"{conclusion}\",\"steps\":[{steps}]}}",
        iso(now, k),
        serde_json::to_string(name).unwrap()
    )
}

fn py_job(id: u64, now: DateTime<Utc>, k: u64, py: u64) -> String {
    let odd = py % 2 == 1;
    let ok = 3 - py % 2;
    let queued = 2 + py % 2;
    let name = if odd { "Prep tests" } else { "Run the tests" };
    let mut steps = String::new();
    for _ in 0..ok {
        steps.push_str("{\"status\":\"completed\",\"conclusion\":\"success\"},");
    }
    steps.push_str("{\"status\":\"skipped\"},");
    steps.push_str(&format!(
        "{{\"name\":\"{name}\",\"status\":\"in_progress\"}}"
    ));
    for _ in 0..queued {
        steps.push_str(",{\"status\":\"queued\"}");
    }
    format!(
        "{{\"id\":{id},\"created_at\":\"{}\",\"name\":\"Test suite Py 3.{py}\",\
         \"status\":\"in_progress\",\"conclusion\":\"\",\"steps\":[{steps}]}}",
        iso(now, k)
    )
}

fn jobs(jobs: Vec<String>) -> String {
    format!("{{\"jobs\":[{}]}}", jobs.join(","))
}

fn iso(now: DateTime<Utc>, k: u64) -> String {
    (now - chrono::Duration::milliseconds((k as i64) * 100))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

const EXPECTED: &str = r###"
fix: most awesome fix nedbat/test [push]   4b2ff5812479  @04:55pm
   ↻ in_progress  Test suite         view 123456789
      A delayed job                  ⏲ queued
      A failed job                   ✗ failure Didn't work
      A finished job                 ✓ success
      Test suite Py 3.8              ↻ •••◦••• Run the tests
      Test suite Py 3.9              ↻ ••◦•••• Prep tests
      Test suite Py 3.10             ↻ •••◦••• Run the tests
      Test suite Py 3.11             ↻ ••◦•••• Prep tests
   ↻ in_progress  Malicious  data   view 123456789
      A [1mMALICIOUS[0m job        ⏲ queued
   ✓ success      A success run      view 123456789
   ✗ startup_failure A startup_failure run   view 123456789
      Just one job                   ⏲ queued
   ⏭ skipped      A skipped run      view 123456789
   ✗ failure      A failure run      view 123456789
      Just one job                   ⏲ queued
   † cancelled    A cancelled run    view 123456789
      Just one job                   ⏲ queued
"###;

fn tz_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[test]
fn draws_the_sample_data() {
    let _guard = tz_lock().lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        std::env::set_var("TZ", "UTC");
    }

    let now = now_utc();
    let now_local = now.with_timezone(&Local);
    let urls = vec!["demo:one".to_string()];
    let mut bucketer = Bucketer::new(5.0);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let events = runtime
        .block_on(get_events(now, &urls, &Fake, None, &mut bucketer))
        .unwrap();

    let mut out = String::new();
    let o = draw_events(&Renderer::PLAIN, now_local, &events, &mut out);

    let expected = EXPECTED.trim_start();
    assert_eq!(out, expected);
    assert!(!o.done);
    assert!(!o.succeeded);
    assert_eq!(o.total, 13);
    assert_eq!(o.num_succeeded, 1);
    assert_eq!(o.num_failed, 1);
}
