use rand::Rng;
use std::collections::HashMap;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};

// Job struct to hold the channel to receive results
struct Job {
    rx: mpsc::Receiver<String>,
}

// JobManager struct that manages all jobs
pub struct JobManager {
    jobs: HashMap<u64, Job>,
}

impl JobManager {
    // Constructor to create a new JobManager
    pub fn new() -> Self {
        Self {
            jobs: HashMap::new(),
        }
    }

    // Create a new job that will complete after a random time
    pub async fn create_job(&mut self, id: u64) {
        let (tx, rx) = mpsc::channel(1);

        let job = Job { rx };
        self.jobs.insert(id, job);

        // Spawn the job's async task
        tokio::spawn(async move {
            let duration = Duration::from_secs(rand::thread_rng().gen_range(1..=5));
            sleep(duration).await;
            let result = format!("Job {} completed!", id);
            let _ = tx.send(result).await;
        });

        println!("Created job {}", id);
    }

    // Check all jobs for completion
    pub async fn check_jobs(&mut self) {
        let mut completed = Vec::new();

        for (id, job) in self.jobs.iter_mut() {
            if let Ok(result) = job.rx.try_recv() {
                println!("{}", result);
                completed.push(*id);
            }
        }

        // Remove completed jobs
        for id in completed {
            self.jobs.remove(&id);
        }
    }

    // Check if there are no remaining jobs
    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }

    // Run the job manager (waits for all jobs to complete)
    pub async fn run(&mut self) {
        // Main loop: check jobs until all are completed
        loop {
            sleep(Duration::from_secs(1)).await;
            self.check_jobs().await;

            if self.is_empty() {
                println!("All jobs completed!");
                break;
            }
        }
    }
}

#[tokio::main]
async fn main() {
    let mut manager = JobManager::new();

    // Create N random jobs
    let n = 5;
    for i in 0..n {
        // Move the delay here before job creation
        let delay = Duration::from_secs(rand::thread_rng().gen_range(1..=3));
        sleep(delay).await;

        manager.create_job(i).await;
    }

    // Call the run method to start the waiting/checking process
    manager.run().await;
}
