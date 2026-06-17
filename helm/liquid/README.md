# Liquid Helm Chart

This chart deploys the Liquid backend and dashboard images published to GHCR.
It supports either a built-in PostgreSQL StatefulSet or a previously deployed
PostgreSQL database.

## Built-In PostgreSQL

```bash
helm install liquid ./helm/liquid \
  --set secrets.encryptionKey=replace-with-a-real-secret \
  --set frontend.apiBaseUrl=http://localhost:3001
```

## External PostgreSQL

```bash
helm install liquid ./helm/liquid \
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
kubectl create secret generic liquid-database \
  --from-literal=database-url='postgres://liquid:password@postgres.example.internal:5432/liquid?sslmode=require'

helm install liquid ./helm/liquid \
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
