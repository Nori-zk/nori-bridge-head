use rand::Rng;
use std::collections::HashMap;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};

enum Command {
    CreateJob { id: u64, description: String },
}

struct Job {
    rx: mpsc::Receiver<String>,
}

struct JobManager {
    jobs: HashMap<u64, Job>,
    receiver: mpsc::Receiver<Command>,
    last_job_count: usize,
}

impl JobManager {
    async fn run(mut self) {
        loop {
            // Try to receive a new command without blocking
            match self.receiver.try_recv() {
                Ok(cmd) => match cmd {
                    Command::CreateJob { id, description } => {
                        self.create_job(id, description).await;
                    }
                },
                Err(mpsc::error::TryRecvError::Empty) => {
                    // No new commands, that's fine
                }
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    // Channel closed, but we'll keep running to finish existing jobs
                }
            }

            // Check for completed jobs
            self.check_jobs().await;

            // Yield to other tasks instead of sleeping
            tokio::task::yield_now().await;
        }
    }

    async fn create_job(&mut self, id: u64, description: String) {
        let (tx, rx) = mpsc::channel(1);
        self.jobs.insert(id, Job { rx });

        let description_clone = description.clone();

        
        tokio::spawn(async move {
            let delay = Duration::from_secs(rand::thread_rng().gen_range(1..=5));
            sleep(delay).await;
            let blocking_result = tokio::task::spawn_blocking(move || {
                1000
            }).await.unwrap();
            let result: String = format!("Job {} ({}) completed! Some blocking junk output {}", id, description, blocking_result);
            let _ = tx.send(result).await;
        });

        println!(
            "JobManager: Created job {} with description '{}'",
            id, description_clone
        );
    }

    async fn check_jobs(&mut self) {
        let mut completed = Vec::new();
        for (&id, job) in self.jobs.iter_mut() {
            if let Ok(result) = job.rx.try_recv() {
                println!("{}", result);
                completed.push(id);
            }
        }
        for id in completed {
            self.jobs.remove(&id);
        }

        let current_count = self.jobs.len();
        if current_count != self.last_job_count {
            println!("Jobs still running: {}", current_count);
            self.last_job_count = current_count;
        }
    }
}

#[derive(Clone)]
struct JobManagerHandle {
    sender: mpsc::Sender<Command>,
}

impl JobManagerHandle {
    async fn create_job(
        &self,
        id: u64,
        description: String,
    ) -> Result<(), mpsc::error::SendError<Command>> {
        self.sender
            .send(Command::CreateJob { id, description })
            .await
    }
}

fn spawn_job_manager() -> JobManagerHandle {
    let (tx, rx) = mpsc::channel(32);
    let manager = JobManager {
        jobs: HashMap::new(),
        receiver: rx,
        last_job_count: 0,
    };
    tokio::spawn(manager.run());
    JobManagerHandle { sender: tx }
}

#[tokio::main]
async fn main() {
    let manager_handle = spawn_job_manager();

    // Create initial batch of jobs
    for i in 0..5 {
        let description = format!("Task for job {}", i);
        if let Err(e) = manager_handle.create_job(i, description).await {
            eprintln!("Failed to create job {}: {}", i, e);
        }
    }

    // Add more jobs
    for i in 5..8 {
        let description = format!("Late task for job {}", i);
        if let Err(e) = manager_handle.create_job(i, description).await {
            eprintln!("Failed to create job {}: {}", i, e);
        }
    }

    // Keep the program running to see all jobs complete
    println!("Waiting for exit");
    sleep(Duration::from_secs(10)).await;
    println!("Exiting");
}
