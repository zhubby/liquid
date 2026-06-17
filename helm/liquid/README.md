# Liquid Helm Chart

This chart deploys the Liquid backend and dashboard images published to GHCR.
It supports either a built-in PostgreSQL StatefulSet or a previously deployed
PostgreSQL database.

## Prerequisites

- Helm 3.
- `kubectl` access to a Kubernetes cluster.
- Pull access to `ghcr.io/zhubby/liquid` and `ghcr.io/zhubby/liquid-ui`.
- A real `secrets.encryptionKey` value outside local development.

## Built-In PostgreSQL

```bash
helm upgrade --install liquid ./helm/liquid \
  --namespace liquid --create-namespace \
  --set secrets.encryptionKey=replace-with-a-real-secret \
  --set postgresql.auth.password=replace-with-postgres-password \
  --set frontend.apiBaseUrl=http://localhost:3001
```

## External PostgreSQL

```bash
helm upgrade --install liquid ./helm/liquid \
  --namespace liquid --create-namespace \
  --set postgresql.enabled=false \
  --set externalDatabase.enabled=true \
  --set externalDatabase.host=postgres.example.internal \
  --set externalDatabase.username=liquid \
  --set externalDatabase.password=replace-with-db-password \
  --set externalDatabase.database=liquid \
  --set secrets.encryptionKey=replace-with-a-real-secret
```

For production, prefer a Secret containing the full PostgreSQL URL:

```bash
kubectl create namespace liquid

kubectl -n liquid create secret generic liquid-database \
  --from-literal=database-url='postgres://liquid:password@postgres.example.internal:5432/liquid?sslmode=require'

helm upgrade --install liquid ./helm/liquid \
  --namespace liquid \
  --set postgresql.enabled=false \
  --set database.existingSecret=liquid-database \
  --set database.existingSecretKey=database-url \
  --set secrets.encryptionKey=replace-with-a-real-secret
```

## Images

Defaults use the chart appVersion as the image tag:

- `ghcr.io/zhubby/liquid:v0.2.0`
- `ghcr.io/zhubby/liquid-ui:v0.2.0`

Override `backend.image.tag` and `frontend.image.tag` when deploying another
release tag.

## Local Chart Tests

```bash
helm lint helm/liquid
helm template liquid helm/liquid --namespace liquid > /tmp/liquid-default.yaml
kubectl apply --dry-run=client --validate=false -f /tmp/liquid-default.yaml
helm package helm/liquid --destination /tmp
```

Test external database rendering:

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

## Local Cluster Smoke Test

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

Then check `http://127.0.0.1:3001/healthz` and open
`http://127.0.0.1:3000`.
