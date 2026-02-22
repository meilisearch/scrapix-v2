---
title: "Deploy on Scaleway Kapsule (Kubernetes)"
description: "Full guide to deploying Scrapix on a Scaleway Kapsule Kubernetes cluster"
---

# Deploy on Scaleway Kapsule

This guide walks through deploying Scrapix on [Scaleway Kapsule](https://www.scaleway.com/en/kubernetes-kapsule/), a managed Kubernetes service. All infrastructure (Redpanda, PostgreSQL, Meilisearch, ClickHouse, DragonflyDB) runs as StatefulSets inside the cluster.

> **Cost note:** This setup requires 5+ always-on nodes with LoadBalancer and PVCs. Expect ~€165/month minimum even at zero traffic. For a cheaper alternative, see the [Fly.io deployment](/deploy-flyio).

## Prerequisites

- [Scaleway CLI (`scw`)](https://github.com/scaleway/scaleway-cli) configured with your credentials
- `kubectl` installed and configured
- `helm` v3 installed
- Docker for building images
- A Scaleway Container Registry namespace

## 1. Create the Kapsule Cluster

Create a cluster with three node pools optimized for different workloads:

```bash
# Create the cluster
scw k8s cluster create \
  name=scrapix \
  version=1.30 \
  cni=cilium \
  region=fr-par

# Save the cluster ID
CLUSTER_ID=$(scw k8s cluster list name=scrapix -o json | jq -r '.[0].id')

# Create node pools
# Infra pool: databases and message queue (needs memory + disk I/O)
scw k8s pool create \
  cluster-id=$CLUSTER_ID \
  name=infra \
  node-type=PRO2-S \
  size=3 \
  min-size=3 \
  max-size=3

# Workers pool: crawlers and content processors (CPU-bound, autoscaled)
scw k8s pool create \
  cluster-id=$CLUSTER_ID \
  name=workers \
  node-type=DEV1-M \
  size=2 \
  min-size=0 \
  max-size=10 \
  autoscaling=true

# System pool: API, console, ingress (light workloads)
scw k8s pool create \
  cluster-id=$CLUSTER_ID \
  name=system \
  node-type=DEV1-S \
  size=2 \
  min-size=2 \
  max-size=3
```

Install the kubeconfig:

```bash
scw k8s kubeconfig install $CLUSTER_ID
```

## 2. Install Cluster Dependencies

### Nginx Ingress Controller

```bash
helm repo add ingress-nginx https://kubernetes.github.io/ingress-nginx
helm install ingress-nginx ingress-nginx/ingress-nginx \
  --namespace ingress-nginx --create-namespace \
  --set controller.service.type=LoadBalancer
```

### cert-manager (for Let's Encrypt TLS)

```bash
helm repo add jetstack https://charts.jetstack.io
helm install cert-manager jetstack/cert-manager \
  --namespace cert-manager --create-namespace \
  --set crds.enabled=true
```

### KEDA (for Kafka lag-based autoscaling)

```bash
helm repo add kedacore https://kedacore.github.io/charts
helm install keda kedacore/keda \
  --namespace keda --create-namespace
```

## 3. Build and Push Images

Push images to your Scaleway Container Registry:

```bash
REGISTRY=rg.fr-par.scw.cloud/scrapix

# Build all Rust service images
for target in scrapix-api scrapix-frontier-service scrapix-worker-crawler scrapix-worker-content; do
  docker build --target $target -t $REGISTRY/$target:latest .
  docker push $REGISTRY/$target:latest
done

# Build console image
docker build -t $REGISTRY/console:latest ./console
docker push $REGISTRY/console:latest
```

Create the image pull secret in the cluster:

```bash
kubectl create namespace scrapix

kubectl create secret docker-registry ghcr-pull-secret \
  --docker-server=rg.fr-par.scw.cloud \
  --docker-username=<scw-access-key> \
  --docker-password=<scw-secret-key> \
  -n scrapix
```

## 4. Configure Secrets

Create production secrets (replace placeholder values):

```bash
kubectl create secret generic scrapix-secrets -n scrapix \
  --from-literal=MEILISEARCH_API_KEY="$(openssl rand -hex 32)" \
  --from-literal=POSTGRES_PASSWORD="$(openssl rand -hex 24)" \
  --from-literal=CLICKHOUSE_PASSWORD="$(openssl rand -hex 24)" \
  --from-literal=JWT_SECRET="$(openssl rand -hex 32)"
```

## 5. Deploy with Kustomize

The Scaleway overlay applies all necessary patches for Scaleway Kapsule:

```bash
kubectl apply -k deploy/kubernetes/overlays/scaleway
```

This deploys:
- **Namespace:** `scrapix`
- **Infrastructure:** Redpanda (3 replicas), PostgreSQL, Meilisearch, ClickHouse, DragonflyDB
- **Services:** API (2 replicas), Frontier, Crawler workers (5), Content workers (3), Console (2)
- **Networking:** Ingress with TLS via cert-manager
- **Autoscaling:** HPA for API, KEDA ScaledObjects for workers (Kafka lag-based)
- **Reliability:** Pod Disruption Budgets for all components

## 6. Verify Deployment

```bash
# Check all pods are running
kubectl get pods -n scrapix

# Check services
kubectl get svc -n scrapix

# Check ingress and TLS certificate
kubectl get ingress -n scrapix
kubectl get certificate -n scrapix

# Check PVCs are bound
kubectl get pvc -n scrapix

# Check API health
kubectl port-forward -n scrapix svc/scrapix-api 8080:8080
curl http://localhost:8080/health
```

## 7. DNS Configuration

Point your domain to the Scaleway LoadBalancer IP:

```bash
# Get the LoadBalancer external IP
kubectl get svc -n ingress-nginx ingress-nginx-controller -o jsonpath='{.status.loadBalancer.ingress[0].ip}'
```

Create DNS records:
- `api.scrapix.yourdomain.com` → A record → LoadBalancer IP
- `console.scrapix.yourdomain.com` → A record → LoadBalancer IP

The ingress and cert-manager will handle TLS automatically once DNS propagates.

## Architecture Details

### Node Pool Assignment

The Scaleway overlay uses `nodeSelector` to pin workloads to the right pools:

| Pool | Node Type | Workloads |
|------|-----------|-----------|
| `infra` | PRO2-S (4 vCPU, 16GB) | Redpanda, PostgreSQL, Meilisearch, ClickHouse, DragonflyDB |
| `workers` | DEV1-M (3 vCPU, 4GB) | Frontier, Crawler workers, Content workers |
| `system` | DEV1-S (2 vCPU, 2GB) | API, Console, Ingress controller |

### Resource Allocation

| Component | Replicas | CPU Request | Memory Request | CPU Limit | Memory Limit |
|-----------|----------|-------------|----------------|-----------|--------------|
| API | 2 | 250m | 256Mi | 1000m | 1Gi |
| Frontier | 1 | 250m | 512Mi | 1000m | 2Gi |
| Crawler Workers | 5 | 500m | 512Mi | 2000m | 2Gi |
| Content Workers | 3 | 250m | 256Mi | 1000m | 1Gi |
| Console | 2 | 100m | 128Mi | 500m | 512Mi |
| Redpanda | 3 | 1000m | 2Gi | 4000m | 8Gi |
| Meilisearch | 1 | 500m | 1Gi | 2000m | 4Gi |
| PostgreSQL | 1 | 500m | 512Mi | 2000m | 2Gi |
| ClickHouse | 1 | 500m | 1Gi | 2000m | 4Gi |
| DragonflyDB | 1 | 250m | 512Mi | 1000m | 2Gi |

### KEDA Autoscaling

Workers scale based on Kafka consumer group lag:

| Component | Min | Max | Lag Threshold | Scale-up | Scale-down |
|-----------|-----|-----|---------------|----------|------------|
| Crawler | 0 | 20 | 100 messages | 5 pods/30s | 25%/60s |
| Content | 0 | 10 | 100 messages | 3 pods/30s | 25%/60s |
| Frontier | 0 | 3 | 500 messages | 1 pod/30s | 1 pod/60s |

When there are no messages to process, KEDA scales workers to zero. When a crawl job starts and messages appear, workers scale up within ~30 seconds.

### Storage

| Component | Size | Storage Class |
|-----------|------|---------------|
| Redpanda | 100Gi | scw-bssd |
| Meilisearch | 100Gi | scw-bssd |
| PostgreSQL | 20Gi | scw-bssd |
| ClickHouse | 50Gi | scw-bssd |

Storage uses Scaleway Block Storage SSD with `WaitForFirstConsumer` volume binding and `Retain` reclaim policy.

## Known Issues & Workarounds

### PVC `lost+found` Directory Conflicts

Scaleway block storage volumes contain a `lost+found` directory that conflicts with PostgreSQL and Meilisearch data directories. The `statefulset-fixes.yaml` patch addresses this by using subdirectories:

- **PostgreSQL:** `PGDATA=/var/lib/postgresql/data/pgdata`
- **Meilisearch:** `MEILI_DB_PATH=/meili_data/db`

### fsGroup Permissions

StatefulSet pods need explicit `fsGroup` in their security context for Scaleway PVCs:

- Redpanda: `fsGroup: 101`
- Meilisearch: `fsGroup: 1000`
- PostgreSQL: `fsGroup: 999`

These are applied via the `statefulset-fixes.yaml` patch.

### Redpanda Pod Anti-Affinity

The Scaleway overlay enforces **required** pod anti-affinity for Redpanda replicas, ensuring each replica runs on a different node. This requires at least 3 nodes in the `infra` pool.

## Scaling

### Manual Scaling

```bash
# Scale crawler workers
kubectl scale deployment scrapix-worker-crawler -n scrapix --replicas=10

# Scale API replicas
kubectl scale deployment scrapix-api -n scrapix --replicas=4
```

### Adjusting KEDA Triggers

Edit the ScaledObjects in `deploy/kubernetes/overlays/scaleway/keda/` to change lag thresholds, min/max replicas, or cooldown periods.

## Monitoring

Deploy the monitoring stack (Prometheus + Grafana) alongside the cluster:

```bash
cd deploy/monitoring
docker compose up -d
```

Or install the kube-prometheus-stack for in-cluster monitoring:

```bash
helm repo add prometheus-community https://prometheus-community.github.io/helm-charts
helm install monitoring prometheus-community/kube-prometheus-stack \
  --namespace monitoring --create-namespace
```

All Scrapix Rust services expose Prometheus metrics on their HTTP port at `/metrics`.

## Teardown

```bash
# Delete the Kustomize deployment
kubectl delete -k deploy/kubernetes/overlays/scaleway

# Delete the cluster (removes all node pools and PVCs)
scw k8s cluster delete $CLUSTER_ID with-additional-resources=true

# Optionally delete the container registry
scw registry namespace delete scrapix
```

## Cost Breakdown

| Resource | Monthly Cost (approx.) |
|----------|----------------------|
| 3x PRO2-S nodes (infra) | ~€90 |
| 2x DEV1-M nodes (workers) | ~€24 |
| 2x DEV1-S nodes (system) | ~€14 |
| LoadBalancer | ~€10 |
| Block Storage (270Gi total) | ~€27 |
| **Total (idle)** | **~€165** |

Worker nodes scale with KEDA + cluster autoscaler, so costs increase only during active crawls.
