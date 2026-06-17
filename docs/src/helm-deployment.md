# Helm Deployment

The `helm/liquid` chart deploys two GHCR images:

- `ghcr.io/zhubby/liquid:<tag>` for the Rust API,
- `ghcr.io/zhubby/liquid-ui:<tag>` for the Next.js dashboard.

The chart defaults to `.Chart.AppVersion` as both image tags. For the `0.2.0`
release that means `v0.2.0`.

## Prerequisites

- Helm 3.
- `kubectl` access to a Kubernetes cluster.
- A namespace for Liquid.
- Pull access to the GHCR images.
- A real `LIQUID_ENCRYPTION_KEY` value for non-local deployments.

Create the namespace:

```bash
kubectl create namespace liquid
```

If the GHCR packages require authentication in your cluster, create a pull
secret and pass it through `imagePullSecrets`:

```bash
kubectl -n liquid create secret docker-registry ghcr \
  --docker-server=ghcr.io \
  --docker-username=YOUR_GITHUB_USER \
  --docker-password=YOUR_GITHUB_TOKEN

helm upgrade --install liquid ./helm/liquid \
  --namespace liquid \
  --set 'imagePullSecrets[0].name=ghcr'
```

## Deploy with Built-In PostgreSQL

Built-in PostgreSQL is enabled by default. It creates a single-replica
StatefulSet, Service, Secret, and persistent volume claim.

```bash
helm upgrade --install liquid ./helm/liquid \
  --namespace liquid \
  --set secrets.encryptionKey=replace-with-a-real-secret \
  --set postgresql.auth.password=replace-with-postgres-password \
  --set frontend.apiBaseUrl=http://localhost:3001
```

Use this mode for local clusters and simple environments. For production,
prefer a managed PostgreSQL service or a separately operated PostgreSQL release.

Check rollout:

```bash
kubectl -n liquid rollout status deployment/liquid-backend
kubectl -n liquid rollout status deployment/liquid-frontend
kubectl -n liquid rollout status statefulset/liquid-postgresql
```

Port-forward the API and dashboard:

```bash
kubectl -n liquid port-forward svc/liquid-backend 3001:3001
kubectl -n liquid port-forward svc/liquid-frontend 3000:80
```

Then open `http://localhost:3000` and verify the API:

```bash
curl http://127.0.0.1:3001/healthz
```

## Deploy with External PostgreSQL

Use `externalDatabase` when PostgreSQL is already deployed and the chart should
generate the application database URL:

```bash
helm upgrade --install liquid ./helm/liquid \
  --namespace liquid \
  --set postgresql.enabled=false \
  --set externalDatabase.enabled=true \
  --set externalDatabase.host=postgres.example.internal \
  --set externalDatabase.port=5432 \
  --set externalDatabase.username=liquid \
  --set externalDatabase.password=replace-with-db-password \
  --set externalDatabase.database=liquid \
  --set externalDatabase.sslMode=require \
  --set secrets.encryptionKey=replace-with-a-real-secret
```

When database credentials contain URL-reserved characters, prefer an existing
Secret with the full PostgreSQL URL:

```bash
kubectl -n liquid create secret generic liquid-database \
  --from-literal=database-url='postgres://liquid:password@postgres.example.internal:5432/liquid?sslmode=require'

helm upgrade --install liquid ./helm/liquid \
  --namespace liquid \
  --set postgresql.enabled=false \
  --set database.existingSecret=liquid-database \
  --set database.existingSecretKey=database-url \
  --set secrets.encryptionKey=replace-with-a-real-secret
```

You can also keep application secrets in an existing Secret:

```bash
kubectl -n liquid create secret generic liquid-app-secrets \
  --from-literal=liquid-encryption-key='replace-with-a-real-secret' \
  --from-literal=openai-api-key='optional-openai-key'

helm upgrade --install liquid ./helm/liquid \
  --namespace liquid \
  --set postgresql.enabled=false \
  --set database.existingSecret=liquid-database \
  --set secrets.existingSecret=liquid-app-secrets
```

## Ingress and Browser API URL

The frontend calls the browser-reachable backend URL. Keep these values aligned:

- `backend.corsOrigin`: the dashboard origin allowed by the API.
- `frontend.apiBaseUrl`: the API origin used by the dashboard.
- `backend.ingress.*` and `frontend.ingress.*`: optional public routes.

Example:

```bash
helm upgrade --install liquid ./helm/liquid \
  --namespace liquid \
  --set postgresql.enabled=false \
  --set database.existingSecret=liquid-database \
  --set secrets.existingSecret=liquid-app-secrets \
  --set backend.corsOrigin=https://liquid.example.com \
  --set frontend.apiBaseUrl=https://liquid-api.example.com \
  --set frontend.ingress.enabled=true \
  --set 'frontend.ingress.hosts[0].host=liquid.example.com' \
  --set backend.ingress.enabled=true \
  --set 'backend.ingress.hosts[0].host=liquid-api.example.com'
```

The published Next.js image may have `NEXT_PUBLIC_API_BASE_URL` baked at build
time. For production releases, make sure the GHCR frontend image was built with
the same browser-reachable API URL you pass to Helm, or rebuild and publish the
frontend image with the intended value.

## Local Chart Tests

Run static chart checks:

```bash
helm lint helm/liquid
helm template liquid helm/liquid --namespace liquid > /tmp/liquid-default.yaml
kubectl apply --dry-run=client --validate=false -f /tmp/liquid-default.yaml
helm package helm/liquid --destination /tmp
```

Render the external database mode:

```bash
helm template liquid helm/liquid \
  --namespace liquid \
  --set postgresql.enabled=false \
  --set externalDatabase.enabled=true \
  --set externalDatabase.host=postgres.example.internal \
  --set externalDatabase.username=liquid \
  --set externalDatabase.password=secret \
  --set externalDatabase.database=liquid \
  > /tmp/liquid-external-db.yaml

kubectl apply --dry-run=client --validate=false -f /tmp/liquid-external-db.yaml
```

Render the existing database Secret mode:

```bash
helm template liquid helm/liquid \
  --namespace liquid \
  --set postgresql.enabled=false \
  --set database.existingSecret=liquid-database \
  --set database.existingSecretKey=database-url \
  > /tmp/liquid-existing-db-secret.yaml

kubectl apply --dry-run=client --validate=false -f /tmp/liquid-existing-db-secret.yaml
```

Verify invalid database mode fails:

```bash
helm template liquid helm/liquid \
  --namespace liquid \
  --set externalDatabase.enabled=true \
  --set externalDatabase.host=postgres.example.internal
```

This should fail because `postgresql.enabled` and
`externalDatabase.enabled` are mutually exclusive.

## Local Cluster Smoke Test

Use a local cluster such as kind or minikube. This example assumes GHCR images
are pullable by the cluster:

```bash
kubectl create namespace liquid

helm upgrade --install liquid ./helm/liquid \
  --namespace liquid \
  --set secrets.encryptionKey=local-dev-secret-change-me \
  --set postgresql.auth.password=postgres \
  --set frontend.apiBaseUrl=http://localhost:3001

kubectl -n liquid rollout status statefulset/liquid-postgresql
kubectl -n liquid rollout status deployment/liquid-backend
kubectl -n liquid rollout status deployment/liquid-frontend
helm test liquid --namespace liquid
```

In separate terminals:

```bash
kubectl -n liquid port-forward svc/liquid-backend 3001:3001
kubectl -n liquid port-forward svc/liquid-frontend 3000:80
```

Check:

```bash
curl http://127.0.0.1:3001/healthz
open http://127.0.0.1:3000
```

Remove the local release:

```bash
helm uninstall liquid --namespace liquid
kubectl delete namespace liquid
```
