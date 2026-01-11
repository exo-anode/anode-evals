use crate::kubernetes::pod_spec::{build_agent_pod, AgentPodConfig};
use anyhow::{Context, Result};
use k8s_openapi::api::core::v1::Pod;
use kube::{
    api::{Api, DeleteParams, ListParams, LogParams, PostParams},
    Client,
};
use std::time::Duration;
use tokio::time::{interval, timeout};
use tracing::{debug, error, info, warn};

/// Status of a running agent pod
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PodStatus {
    Pending,
    Running,
    Succeeded,
    Failed(String),
    Unknown,
}

impl From<&str> for PodStatus {
    fn from(phase: &str) -> Self {
        match phase {
            "Pending" => PodStatus::Pending,
            "Running" => PodStatus::Running,
            "Succeeded" => PodStatus::Succeeded,
            "Failed" => PodStatus::Failed("Pod failed".to_string()),
            _ => PodStatus::Unknown,
        }
    }
}

/// Manages Kubernetes pods for agent evaluation
pub struct PodManager {
    client: Client,
    namespace: String,
}

impl PodManager {
    /// Create a new PodManager
    pub async fn new(namespace: &str) -> Result<Self> {
        let client = Client::try_default()
            .await
            .context("Failed to create Kubernetes client")?;

        Ok(Self {
            client,
            namespace: namespace.to_string(),
        })
    }

    /// Create a new PodManager with a specific client (for testing)
    #[allow(dead_code)]
    pub fn with_client(client: Client, namespace: &str) -> Self {
        Self {
            client,
            namespace: namespace.to_string(),
        }
    }

    /// Spawn a new agent pod
    pub async fn spawn_pod(&self, config: &AgentPodConfig) -> Result<String> {
        let pods: Api<Pod> = Api::namespaced(self.client.clone(), &self.namespace);
        let pod = build_agent_pod(config);
        let pod_name = config.pod_name();

        info!("Creating pod: {}", pod_name);

        pods.create(&PostParams::default(), &pod)
            .await
            .context(format!("Failed to create pod: {}", pod_name))?;

        info!("Pod created: {}", pod_name);
        Ok(pod_name)
    }

    /// Get the status of a pod
    pub async fn get_pod_status(&self, pod_name: &str) -> Result<PodStatus> {
        let pods: Api<Pod> = Api::namespaced(self.client.clone(), &self.namespace);

        let pod = pods
            .get(pod_name)
            .await
            .context(format!("Failed to get pod: {}", pod_name))?;

        let status = pod.status.as_ref().and_then(|s| s.phase.as_deref());

        match status {
            Some(phase) => {
                // Check for container status details
                if let Some(container_statuses) =
                    pod.status.as_ref().and_then(|s| s.container_statuses.as_ref())
                {
                    for cs in container_statuses {
                        if let Some(state) = &cs.state {
                            if let Some(terminated) = &state.terminated {
                                if terminated.exit_code != 0 {
                                    return Ok(PodStatus::Failed(format!(
                                        "Container exited with code {}: {}",
                                        terminated.exit_code,
                                        terminated.reason.clone().unwrap_or_default()
                                    )));
                                }
                            }
                            if let Some(waiting) = &state.waiting {
                                if let Some(reason) = &waiting.reason {
                                    if reason.contains("Err")
                                        || reason.contains("BackOff")
                                        || reason.contains("CrashLoop")
                                    {
                                        return Ok(PodStatus::Failed(format!(
                                            "Container waiting: {}",
                                            reason
                                        )));
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(PodStatus::from(phase))
            }
            None => Ok(PodStatus::Unknown),
        }
    }

    /// Wait for a pod to complete with periodic health checks
    pub async fn wait_for_completion(
        &self,
        pod_name: &str,
        check_interval: Duration,
        max_duration: Duration,
    ) -> Result<PodStatus> {
        let _pods: Api<Pod> = Api::namespaced(self.client.clone(), &self.namespace);

        info!(
            "Waiting for pod {} to complete (max {}s)",
            pod_name,
            max_duration.as_secs()
        );

        let result = timeout(max_duration, async {
            let mut check_ticker = interval(check_interval);

            loop {
                check_ticker.tick().await;

                let status = self.get_pod_status(pod_name).await?;
                debug!("Pod {} status: {:?}", pod_name, status);

                match status {
                    PodStatus::Succeeded => {
                        info!("Pod {} completed successfully", pod_name);
                        return Ok(PodStatus::Succeeded);
                    }
                    PodStatus::Failed(reason) => {
                        error!("Pod {} failed: {}", pod_name, reason);
                        return Ok(PodStatus::Failed(reason));
                    }
                    PodStatus::Pending | PodStatus::Running => {
                        // Continue waiting
                        continue;
                    }
                    PodStatus::Unknown => {
                        warn!("Pod {} has unknown status", pod_name);
                        continue;
                    }
                }
            }
        })
        .await;

        match result {
            Ok(status) => status,
            Err(_) => {
                warn!("Pod {} timed out after {:?}", pod_name, max_duration);
                Ok(PodStatus::Failed("Timeout".to_string()))
            }
        }
    }

    /// Get logs from a pod
    pub async fn get_pod_logs(&self, pod_name: &str) -> Result<String> {
        let pods: Api<Pod> = Api::namespaced(self.client.clone(), &self.namespace);

        let logs = pods
            .logs(
                pod_name,
                &LogParams {
                    container: Some("agent".to_string()),
                    ..Default::default()
                },
            )
            .await
            .context(format!("Failed to get logs for pod: {}", pod_name))?;

        Ok(logs)
    }

    /// Delete a pod
    pub async fn delete_pod(&self, pod_name: &str) -> Result<()> {
        let pods: Api<Pod> = Api::namespaced(self.client.clone(), &self.namespace);

        info!("Deleting pod: {}", pod_name);

        pods.delete(pod_name, &DeleteParams::default())
            .await
            .context(format!("Failed to delete pod: {}", pod_name))?;

        Ok(())
    }

    /// List all pods for a given run
    pub async fn list_run_pods(&self, run_id: &str) -> Result<Vec<String>> {
        let pods: Api<Pod> = Api::namespaced(self.client.clone(), &self.namespace);

        let lp = ListParams::default().labels(&format!("run-id={}", run_id));
        let pod_list = pods.list(&lp).await.context("Failed to list pods")?;

        Ok(pod_list
            .items
            .into_iter()
            .filter_map(|p| p.metadata.name)
            .collect())
    }

    /// Cleanup all pods for a given run
    pub async fn cleanup_run(&self, run_id: &str) -> Result<()> {
        let pod_names = self.list_run_pods(run_id).await?;

        for pod_name in pod_names {
            if let Err(e) = self.delete_pod(&pod_name).await {
                warn!("Failed to delete pod {}: {}", pod_name, e);
            }
        }

        Ok(())
    }

    /// Execute a command in a running pod and get output
    /// This is used to run the eval suite after the agent completes
    pub async fn exec_in_pod(&self, pod_name: &str, command: Vec<String>) -> Result<String> {
        // Note: For now, we'll use kubectl exec via subprocess
        // In production, you'd want to use the kube-rs exec API
        let output = tokio::process::Command::new("kubectl")
            .args(["exec", "-n", &self.namespace, pod_name, "--"])
            .args(&command)
            .output()
            .await
            .context("Failed to execute command in pod")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            Ok(stdout.to_string())
        } else {
            Err(anyhow::anyhow!(
                "Command failed: {}\nstderr: {}",
                stdout,
                stderr
            ))
        }
    }

    /// Copy files from pod to local filesystem
    #[allow(dead_code)]
    pub async fn copy_from_pod(
        &self,
        pod_name: &str,
        pod_path: &str,
        local_path: &str,
    ) -> Result<()> {
        let pod_full_path = format!("{}:{}", pod_name, pod_path);

        let output = tokio::process::Command::new("kubectl")
            .args(["cp", "-n", &self.namespace, &pod_full_path, local_path])
            .output()
            .await
            .context("Failed to copy files from pod")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("kubectl cp failed: {}", stderr));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pod_status_from_str() {
        assert_eq!(PodStatus::from("Pending"), PodStatus::Pending);
        assert_eq!(PodStatus::from("Running"), PodStatus::Running);
        assert_eq!(PodStatus::from("Succeeded"), PodStatus::Succeeded);
        assert_eq!(
            PodStatus::from("Failed"),
            PodStatus::Failed("Pod failed".to_string())
        );
        assert_eq!(PodStatus::from("Unknown"), PodStatus::Unknown);
    }
}
