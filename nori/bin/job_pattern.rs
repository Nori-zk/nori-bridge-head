use rand::Rng;
use std::collections::HashMap;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};

// Struct to hold job status and channel to receive result
struct Job {
    rx: mpsc::Receiver<String>,
}

struct JobManager {
    jobs: HashMap<u64, Job>,
}

impl JobManager {
    fn new() -> Self {
        Self {
            jobs: HashMap::new(),
        }
    }

    // Create a new job that will complete after random time
    async fn create_job(&mut self, id: u64) {
        // Random delay before creating the job
        let delay = Duration::from_secs(rand::thread_rng().gen_range(1..=3));
        sleep(delay).await;

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
    async fn check_jobs(&mut self) {
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
}

#[tokio::main]
async fn main() {
    let mut manager = JobManager::new();

    // Create N random jobs
    let n = 5;
    for i in 0..n {
        manager.create_job(i).await;
    }

    // Main loop: check jobs forever
    loop {
        sleep(Duration::from_secs(1)).await;
        manager.check_jobs().await;

        if manager.jobs.is_empty() {
            println!("All jobs completed!");
            break;
        }
    }
}
