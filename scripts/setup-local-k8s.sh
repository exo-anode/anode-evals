#!/bin/bash
# Setup script for local Kubernetes cluster using kind (Kubernetes in Docker)

set -e

echo "=== ANODE-EVAL Local Kubernetes Setup ==="

# Check if kind is installed
if ! command -v kind &> /dev/null; then
    echo "Installing kind..."
    if [[ "$OSTYPE" == "darwin"* ]]; then
        brew install kind
    else
        curl -Lo ./kind https://kind.sigs.k8s.io/dl/v0.20.0/kind-linux-amd64
        chmod +x ./kind
        sudo mv ./kind /usr/local/bin/kind
    fi
fi

# Check if kubectl is installed
if ! command -v kubectl &> /dev/null; then
    echo "Installing kubectl..."
    if [[ "$OSTYPE" == "darwin"* ]]; then
        brew install kubectl
    else
        curl -LO "https://dl.k8s.io/release/$(curl -L -s https://dl.k8s.io/release/stable.txt)/bin/linux/amd64/kubectl"
        chmod +x kubectl
        sudo mv kubectl /usr/local/bin/kubectl
    fi
fi

# Check if cluster already exists
if kind get clusters 2>/dev/null | grep -q "anode-eval"; then
    echo "Cluster 'anode-eval' already exists"
else
    echo "Creating kind cluster 'anode-eval'..."
    kind create cluster --name anode-eval --config - <<EOF
kind: Cluster
apiVersion: kind.x-k8s.io/v1alpha4
nodes:
  - role: control-plane
  - role: worker
  - role: worker
EOF
fi

# Set kubectl context
kubectl cluster-info --context kind-anode-eval

# Create namespace
echo "Creating namespace..."
kubectl apply -f k8s/namespace.yaml

# Apply RBAC
echo "Setting up RBAC..."
kubectl apply -f k8s/rbac.yaml

# Build and load the agent image
echo "Building agent Docker image..."
docker build -t anode-eval-agent:latest -f k8s/Dockerfile.agent .

echo "Loading image into kind cluster..."
kind load docker-image anode-eval-agent:latest --name anode-eval

echo ""
echo "=== Setup Complete ==="
echo "Cluster: anode-eval"
echo "Namespace: anode-eval"
echo ""
echo "To run an evaluation:"
echo "  cargo run -- run --config examples/hello_world/eval-config.yaml"
