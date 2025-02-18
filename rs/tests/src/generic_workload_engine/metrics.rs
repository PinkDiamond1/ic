use std::fmt::Display;
use std::{
    cmp::{max, min},
    collections::{BTreeMap, HashMap},
    time::Duration,
};

// TODO
// #[derive(Default, Clone, Debug)]
// pub struct RequestDurationBucket {
//     threshold: Duration,
//     requests_above_threshold: u64,
//     requests_below_threshold: u64,
// }

pub type Counter = usize;

#[derive(Debug)]
pub struct RequestMetrics {
    errors_max: Counter,
    errors_map: HashMap<String, Counter>,
    min_attempts: Counter,
    total_attempts: Counter,
    max_attempts: Counter,
    success_calls: Counter,
    failure_calls: Counter,
    min_request_duration: Duration,
    max_request_duration: Duration,
    total_request_duration: Duration,
    //requests_duration_buckets: Option<Vec<RequestDurationBucket>>, TODO
}

/// Outcome of a request-based workflow, i.e., r_1, r_2, ..., r_N in which each individual request r_i may depend on the outcome of r_{i-1}
pub type LoadTestOutcome<T, S> = Vec<(String, RequestOutcome<T, S>)>;

#[derive(Default)]
pub struct LoadTestMetrics {
    /// Keys are (request_pos, request_label) pairs, where
    /// - request_pos is the position of the request in the workflow (see [`LoadTestOutcome`])
    /// - request_label is a request label
    inner: BTreeMap<String, RequestMetrics>,
}

impl LoadTestMetrics {
    pub fn aggregate_load_testing_metrics<T, S>(mut self, item: LoadTestOutcome<T, S>) -> Self
    where
        T: Clone,
        S: Clone + Display,
    {
        item.into_iter().for_each(|(req_name, outcome)| {
            let entry = self.inner.entry(req_name).or_default();
            entry.push(outcome)
        });
        self
    }

    pub fn aggregator_fn(
        aggr: LoadTestMetrics,
        item: LoadTestOutcome<(), String>,
    ) -> LoadTestMetrics {
        aggr.aggregate_load_testing_metrics(item)
    }
}

impl Display for LoadTestMetrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LoadTestMetrics {{ ")
            .and(
                self.inner
                    .iter()
                    .fold(write!(f, ""), |acc, (req_name, metrics)| {
                        acc.and(write!(f, "  {req_name}: "))
                            .and(metrics.fmt(f))
                            .and(write!(f, ", "))
                    }),
            )
            .and(write!(f, "}}"))
    }
}

impl RequestMetrics {
    pub fn total_calls(&self) -> Counter {
        self.success_calls + self.failure_calls
    }

    pub fn success_calls(&self) -> Counter {
        self.success_calls
    }

    pub fn failure_calls(&self) -> Counter {
        self.failure_calls
    }

    pub fn min_request_duration(&self) -> Duration {
        self.min_request_duration
    }

    pub fn avg_request_duration(&self) -> Duration {
        self.total_request_duration
            .checked_div(self.total_calls().try_into().unwrap())
            .unwrap()
    }

    pub fn success_rate(&self) -> f64 {
        (100 * self.success_calls()) as f64 / (self.total_calls() as f64)
    }

    pub fn max_request_duration(&self) -> Duration {
        self.max_request_duration
    }

    pub fn min_attempts(&self) -> Counter {
        self.min_attempts
    }

    pub fn max_attempts(&self) -> Counter {
        self.max_attempts
    }

    pub fn avg_attempts(&self) -> Option<f64> {
        let tot = self.total_calls();
        if tot == 0 {
            None
        } else {
            Some((self.total_attempts as f64) / (tot as f64))
        }
    }

    pub fn errors(&self) -> &HashMap<String, Counter> {
        &self.errors_map
    }

    pub fn success_ratio(&self) -> f64 {
        self.success_calls as f64 / self.total_calls() as f64
    }

    pub fn failure_ratio(&self) -> f64 {
        1.0 - self.success_ratio()
    }

    pub fn push<T, S>(&mut self, item: RequestOutcome<T, S>)
    where
        T: Clone,
        S: Clone + ToString,
    {
        self.min_request_duration = min(self.min_request_duration, item.duration);
        self.max_request_duration = max(self.max_request_duration, item.duration);
        self.total_request_duration += item.duration;

        self.min_attempts = min(self.min_attempts, item.attempts);
        self.max_attempts = max(self.max_attempts, item.attempts);
        self.total_attempts += item.attempts;

        if let Err(error) = item.result {
            self.failure_calls += 1;
            *self.errors_map.entry(error.to_string()).or_insert(0) += 1;
            if self.errors_map.len() > self.errors_max {
                panic!(
                    "Hash table holding errors exceeded predefined max_size={}.",
                    self.errors_max
                );
            }
        } else {
            self.success_calls += 1;
        }
    }
}

impl Default for RequestMetrics {
    fn default() -> Self {
        let errors_max = 10_000;
        RequestMetrics {
            errors_max,
            errors_map: HashMap::with_capacity(errors_max),
            success_calls: 0,
            failure_calls: 0,
            min_request_duration: Duration::MAX,
            max_request_duration: Duration::default(),
            total_request_duration: Duration::default(),
            // requests_duration_buckets: None, TODO
            min_attempts: Counter::MAX,
            max_attempts: 0,
            total_attempts: 0,
        }
    }
}

impl Display for RequestMetrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "RequestMetrics {{ duration=(min:{}ms, avg:{}ms, max:{}ms), attempts=(min:{}, avg:{}, max:{}), success_rate:{:.2}%, successes:{}, failures:{}, errors:```{:?}``` }}",
            self.min_request_duration().as_millis(),
            self.avg_request_duration().as_millis(),
            self.max_request_duration().as_millis(),
            self.min_attempts(),
            self.avg_attempts().map(|x| format!("{x}")).unwrap_or_else(|| "inf".to_string()),
            self.max_attempts(),
            self.success_rate(),
            self.success_calls(),
            self.failure_calls(),
            self.errors_map
        )
    }
}

/// Outcome of a **generalized** request, e.g.:
/// - A single canister endpoint request (query or update)
/// - An HTTP request that is expected to return a JSON object of a particular shape
///
/// [`ResultType`] can be instantiated with one of the following:
/// 1. Union, i.e., [`()`], meaning that we are solely interested in whether there has been an error, and the result value is not used.
/// 2. A concrete type, e.g., [`Value`] for an http_request that serves a JSON object.
///    This is the preferred case, as the client can then match on the structure of [`ResultType`] without needing to decode it.
/// 3. A generic encoding of a response, i.e., [`Vec<u8>`]. This allows aggregating multiple instances of [`RequestOutcome`],
///    even when they correspond to different requests that return responses of incompatible types. For example, this is needed
///    for collecting metrics in a stateful workload generation scenario, when one first calls request A and then (upon its success)
///    request B, and if type(response(A)) != type(response(B)).
#[derive(Debug, Clone)]
pub struct RequestOutcome<ResultType: Clone, ErrorType: Clone> {
    result: Result<ResultType, ErrorType>,
    /// Each request class can be identified via a [`(workflow_pos, label)`] pair used to aggregate statistical information about outcomes of multiple requests with the same label.
    /// - [`workflow_pos`] is an (optional, unique) position of this request in its workflow. [`None`] is used to classify the overall worflow outcome, in which case the position
    ///   is statically unknown. See [`with_workflow_position`]
    /// - [`label`] is a canister endpoint name, or some other (short) description of the request.
    /// See [`RequestOutcome.into_test_outcome`].
    workflow_pos: Option<usize>,
    label: String,
    pub duration: Duration,
    pub attempts: Counter,
}

impl<ResultType: Clone, ErrorType: Clone> RequestOutcome<ResultType, ErrorType> {
    pub fn new(
        result: Result<ResultType, ErrorType>,
        label: String,
        duration: Duration,
        attempts: Counter,
    ) -> Self {
        Self {
            result,
            workflow_pos: None,
            label,
            duration,
            attempts,
        }
    }

    pub fn result(&self) -> Result<ResultType, ErrorType> {
        self.result.clone()
    }

    pub fn with_workflow_position(mut self, pos: usize) -> Self {
        self.workflow_pos = Some(pos);
        self
    }

    pub fn map<F, NewResultType>(&self, f: F) -> RequestOutcome<NewResultType, ErrorType>
    where
        NewResultType: Clone,
        F: FnOnce(ResultType) -> NewResultType,
    {
        RequestOutcome {
            result: self.result.clone().map(f),
            workflow_pos: self.workflow_pos,
            label: self.label.clone(),
            duration: self.duration,
            attempts: self.attempts,
        }
    }

    pub fn map_err<F, NewErrorType>(&self, f: F) -> RequestOutcome<ResultType, NewErrorType>
    where
        NewErrorType: Clone,
        F: FnOnce(ErrorType) -> NewErrorType,
    {
        RequestOutcome {
            result: self.result.clone().map_err(f),
            workflow_pos: self.workflow_pos,
            label: self.label.clone(),
            duration: self.duration,
            attempts: self.attempts,
        }
    }

    fn key(&self) -> String {
        if let Some(workflow_pos) = self.workflow_pos {
            format!("{workflow_pos}_{}", self.label)
        } else {
            self.label.clone()
        }
    }

    pub fn into_test_outcome(self) -> LoadTestOutcome<ResultType, ErrorType> {
        vec![(self.key(), self)]
    }

    pub fn push_outcome(self, test_outcome: &mut LoadTestOutcome<ResultType, ErrorType>) -> Self {
        test_outcome.push((self.key(), self.clone()));
        self
    }
}
