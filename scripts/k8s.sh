#!/usr/bin/env bash
# Scrapix Kubernetes Deployment Tool
# Manages Kubernetes deployments for testing and production

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
cd "$PROJECT_ROOT"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

# Default settings
NAMESPACE="scrapix"
OVERLAY="local"
ACTION=""
COMPONENT=""
REPLICAS=""
FOLLOW_LOGS=false
WATCH_STATUS=false

usage() {
    cat << EOF
Usage: $(basename "$0") [OPTIONS] <ACTION> [COMPONENT]

Scrapix Kubernetes Deployment Tool

ACTIONS:
    deploy              Deploy all services to Kubernetes
    destroy             Remove all services from Kubernetes
    status              Show deployment status
    logs                Show logs for a component
    scale               Scale a component
    restart             Restart a component
    port-forward        Forward ports for local access
    bench               Run benchmark in Kubernetes

COMPONENTS (for logs, scale, restart):
    api                 API server
    frontier            Frontier service
    crawler             Crawler worker
    content             Content worker
    all                 All components (default for some actions)

OPTIONS:
    -n, --namespace NS  Kubernetes namespace (default: scrapix)
    -o, --overlay NAME  Kustomize overlay: local, staging, prod (default: local)
    -r, --replicas N    Number of replicas (for scale action)
    -f, --follow        Follow logs
    -w, --watch         Watch status continuously
    -h, --help          Show this help

EXAMPLES:
    $(basename "$0") deploy                     # Deploy to local cluster
    $(basename "$0") -o prod deploy             # Deploy to production
    $(basename "$0") logs crawler -f            # Follow crawler logs
    $(basename "$0") scale crawler -r 5         # Scale crawler to 5 replicas
    $(basename "$0") status -w                  # Watch deployment status
    $(basename "$0") port-forward               # Forward all ports locally
    $(basename "$0") bench                      # Run benchmark in cluster

EOF
}

log_info() { echo -e "${BLUE}[INFO]${NC} $*"; }
log_success() { echo -e "${GREEN}[OK]${NC} $*"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $*"; }
log_error() { echo -e "${RED}[ERROR]${NC} $*" >&2; }

parse_args() {
    while [[ $# -gt 0 ]]; do
        case $1 in
            -n|--namespace)
                NAMESPACE="$2"
                shift 2
                ;;
            -o|--overlay)
                OVERLAY="$2"
                shift 2
                ;;
            -r|--replicas)
                REPLICAS="$2"
                shift 2
                ;;
            -f|--follow)
                FOLLOW_LOGS=true
                shift
                ;;
            -w|--watch)
                WATCH_STATUS=true
                shift
                ;;
            -h|--help)
                usage
                exit 0
                ;;
            deploy|destroy|status|logs|scale|restart|port-forward|bench)
                ACTION="$1"
                shift
                ;;
            api|frontier|crawler|content|all)
                COMPONENT="$1"
                shift
                ;;
            *)
                log_error "Unknown option: $1"
                usage
                exit 1
                ;;
        esac
    done

    if [[ -z "$ACTION" ]]; then
        log_error "No action specified"
        usage
        exit 1
    fi
}

check_kubectl() {
    if ! command -v kubectl &> /dev/null; then
        log_error "kubectl not found. Please install kubectl."
        exit 1
    fi

    if ! kubectl cluster-info &> /dev/null; then
        log_error "Cannot connect to Kubernetes cluster. Check your kubeconfig."
        exit 1
    fi
}

get_deployment_name() {
    local component="$1"
    case "$component" in
        api) echo "scrapix-api" ;;
        frontier) echo "scrapix-frontier" ;;
        crawler) echo "scrapix-crawler" ;;
        content) echo "scrapix-content" ;;
        *) echo "scrapix-$component" ;;
    esac
}

do_deploy() {
    log_info "Deploying Scrapix to Kubernetes..."
    log_info "Namespace: $NAMESPACE"
    log_info "Overlay: $OVERLAY"

    local overlay_path="deploy/kubernetes/overlays/$OVERLAY"

    if [[ ! -d "$overlay_path" ]]; then
        log_error "Overlay not found: $overlay_path"
        exit 1
    fi

    # Create namespace if it doesn't exist
    kubectl create namespace "$NAMESPACE" --dry-run=client -o yaml | kubectl apply -f -

    # Apply kustomize
    log_info "Applying kustomize overlay..."
    kubectl apply -k "$overlay_path" -n "$NAMESPACE"

    # Wait for deployments
    log_info "Waiting for deployments to be ready..."
    kubectl rollout status deployment -n "$NAMESPACE" --timeout=300s || true

    log_success "Deployment complete!"
    do_status
}

do_destroy() {
    log_warn "This will delete all Scrapix resources in namespace '$NAMESPACE'"
    read -p "Are you sure? (y/N) " -n 1 -r
    echo

    if [[ $REPLY =~ ^[Yy]$ ]]; then
        log_info "Destroying Scrapix deployment..."
        kubectl delete -k "deploy/kubernetes/overlays/$OVERLAY" -n "$NAMESPACE" --ignore-not-found
        log_success "Resources deleted"
    else
        log_info "Cancelled"
    fi
}

do_status() {
    echo ""
    echo "=== Scrapix Deployment Status ==="
    echo ""

    if [[ "$WATCH_STATUS" == "true" ]]; then
        watch -n 2 "kubectl get pods,svc,deployments -n $NAMESPACE -o wide"
    else
        echo "Deployments:"
        kubectl get deployments -n "$NAMESPACE" -o wide 2>/dev/null || echo "  No deployments found"
        echo ""
        echo "Pods:"
        kubectl get pods -n "$NAMESPACE" -o wide 2>/dev/null || echo "  No pods found"
        echo ""
        echo "Services:"
        kubectl get svc -n "$NAMESPACE" 2>/dev/null || echo "  No services found"
        echo ""

        # Show resource usage
        echo "Resource Usage:"
        kubectl top pods -n "$NAMESPACE" 2>/dev/null || echo "  Metrics not available"
    fi
}

do_logs() {
    local component="${COMPONENT:-all}"

    if [[ "$component" == "all" ]]; then
        log_info "Streaming logs from all components..."
        local follow_flag=""
        [[ "$FOLLOW_LOGS" == "true" ]] && follow_flag="-f"

        # Use stern if available, otherwise kubectl
        if command -v stern &> /dev/null; then
            stern -n "$NAMESPACE" "scrapix" $follow_flag
        else
            kubectl logs -n "$NAMESPACE" -l app.kubernetes.io/name=scrapix --all-containers --prefix $follow_flag
        fi
    else
        local deployment
        deployment=$(get_deployment_name "$component")
        local follow_flag=""
        [[ "$FOLLOW_LOGS" == "true" ]] && follow_flag="-f"

        log_info "Streaming logs from $deployment..."
        kubectl logs -n "$NAMESPACE" "deployment/$deployment" --all-containers $follow_flag
    fi
}

do_scale() {
    local component="${COMPONENT:-crawler}"

    if [[ -z "$REPLICAS" ]]; then
        log_error "Please specify number of replicas with -r/--replicas"
        exit 1
    fi

    local deployment
    deployment=$(get_deployment_name "$component")

    log_info "Scaling $deployment to $REPLICAS replicas..."
    kubectl scale deployment "$deployment" -n "$NAMESPACE" --replicas="$REPLICAS"

    kubectl rollout status deployment "$deployment" -n "$NAMESPACE" --timeout=120s
    log_success "Scaled $deployment to $REPLICAS replicas"
}

do_restart() {
    local component="${COMPONENT:-all}"

    if [[ "$component" == "all" ]]; then
        log_info "Restarting all Scrapix components..."
        kubectl rollout restart deployment -n "$NAMESPACE" -l app.kubernetes.io/name=scrapix
    else
        local deployment
        deployment=$(get_deployment_name "$component")

        log_info "Restarting $deployment..."
        kubectl rollout restart deployment "$deployment" -n "$NAMESPACE"
    fi

    kubectl rollout status deployment -n "$NAMESPACE" --timeout=120s
    log_success "Restart complete"
}

do_port_forward() {
    log_info "Setting up port forwarding..."

    # Kill any existing port forwards
    pkill -f "kubectl port-forward.*scrapix" 2>/dev/null || true

    # API Server
    kubectl port-forward -n "$NAMESPACE" svc/scrapix-api 8080:8080 &
    log_success "API Server: http://localhost:8080"

    # Meilisearch (if in cluster)
    kubectl port-forward -n "$NAMESPACE" svc/meilisearch 7700:7700 2>/dev/null &
    log_success "Meilisearch: http://localhost:7700"

    # Redpanda Console (if in cluster)
    kubectl port-forward -n "$NAMESPACE" svc/redpanda-console 8090:8080 2>/dev/null &
    log_success "Redpanda Console: http://localhost:8090"

    log_info "Port forwards active. Press Ctrl+C to stop."
    wait
}

do_bench() {
    log_info "Running benchmark in Kubernetes cluster..."

    # Create a benchmark job
    cat << EOF | kubectl apply -n "$NAMESPACE" -f -
apiVersion: batch/v1
kind: Job
metadata:
  name: scrapix-benchmark-$(date +%s)
spec:
  ttlSecondsAfterFinished: 3600
  template:
    spec:
      restartPolicy: Never
      containers:
      - name: benchmark
        image: scrapix:latest
        imagePullPolicy: Never
        command: ["/bin/sh", "-c"]
        args:
          - |
            echo "Starting Scrapix Benchmark"
            echo "========================="
            echo ""

            # Run the benchmark
            cargo bench --bench wikipedia_e2e 2>&1

            echo ""
            echo "Benchmark complete!"
        resources:
          requests:
            cpu: "2"
            memory: "4Gi"
          limits:
            cpu: "4"
            memory: "8Gi"
EOF

    log_info "Benchmark job submitted. Following logs..."

    # Wait for pod to start
    sleep 5

    # Follow logs
    kubectl logs -n "$NAMESPACE" -l job-name=scrapix-benchmark --follow
}

main() {
    parse_args "$@"
    check_kubectl

    echo ""
    echo "=========================================="
    echo "   Scrapix Kubernetes Tool"
    echo "=========================================="
    echo ""

    case "$ACTION" in
        deploy) do_deploy ;;
        destroy) do_destroy ;;
        status) do_status ;;
        logs) do_logs ;;
        scale) do_scale ;;
        restart) do_restart ;;
        port-forward) do_port_forward ;;
        bench) do_bench ;;
        *)
            log_error "Unknown action: $ACTION"
            usage
            exit 1
            ;;
    esac
}

main "$@"
