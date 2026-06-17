{{/*
Expand the name of the chart.
*/}}
{{- define "liquid.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Create a default fully qualified app name.
*/}}
{{- define "liquid.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{/*
Create chart name and version as used by the chart label.
*/}}
{{- define "liquid.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Common labels.
*/}}
{{- define "liquid.labels" -}}
helm.sh/chart: {{ include "liquid.chart" . }}
app.kubernetes.io/name: {{ include "liquid.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{/*
Selector labels for a component.
*/}}
{{- define "liquid.selectorLabels" -}}
app.kubernetes.io/name: {{ include "liquid.name" .root }}
app.kubernetes.io/instance: {{ .root.Release.Name }}
app.kubernetes.io/component: {{ .component }}
{{- end }}

{{/*
Common labels plus component.
*/}}
{{- define "liquid.componentLabels" -}}
{{ include "liquid.labels" .root }}
app.kubernetes.io/component: {{ .component }}
{{- end }}

{{- define "liquid.backendFullname" -}}
{{- printf "%s-backend" (include "liquid.fullname" .) | trunc 63 | trimSuffix "-" }}
{{- end }}

{{- define "liquid.frontendFullname" -}}
{{- printf "%s-frontend" (include "liquid.fullname" .) | trunc 63 | trimSuffix "-" }}
{{- end }}

{{- define "liquid.postgresqlFullname" -}}
{{- printf "%s-postgresql" (include "liquid.fullname" .) | trunc 63 | trimSuffix "-" }}
{{- end }}

{{- define "liquid.configName" -}}
{{- printf "%s-config" (include "liquid.fullname" .) | trunc 63 | trimSuffix "-" }}
{{- end }}

{{- define "liquid.secretName" -}}
{{- printf "%s-secret" (include "liquid.fullname" .) | trunc 63 | trimSuffix "-" }}
{{- end }}

{{- define "liquid.postgresqlSecretName" -}}
{{- printf "%s-postgresql" (include "liquid.secretName" .) | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Create the name of the service account to use.
*/}}
{{- define "liquid.serviceAccountName" -}}
{{- if .Values.serviceAccount.create }}
{{- default (include "liquid.fullname" .) .Values.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.serviceAccount.name }}
{{- end }}
{{- end }}

{{/*
Validate database mode.
*/}}
{{- define "liquid.validateDatabase" -}}
{{- if and .Values.postgresql.enabled .Values.externalDatabase.enabled -}}
{{- fail "postgresql.enabled and externalDatabase.enabled are mutually exclusive" -}}
{{- end -}}
{{- if and (not .Values.postgresql.enabled) (not .Values.externalDatabase.enabled) (not .Values.database.existingSecret) -}}
{{- fail "set postgresql.enabled=true, externalDatabase.enabled=true, or database.existingSecret" -}}
{{- end -}}
{{- if and .Values.externalDatabase.enabled (not .Values.database.existingSecret) (not .Values.externalDatabase.host) -}}
{{- fail "externalDatabase.host is required when externalDatabase.enabled=true and database.existingSecret is not set" -}}
{{- end -}}
{{- end }}

{{/*
Build the generated application database URL.
*/}}
{{- define "liquid.databaseUrl" -}}
{{- if .Values.postgresql.enabled -}}
postgres://{{ .Values.postgresql.auth.username }}:{{ .Values.postgresql.auth.password }}@{{ include "liquid.postgresqlFullname" . }}:{{ .Values.postgresql.service.port }}/{{ .Values.postgresql.auth.database }}
{{- else -}}
{{- $host := required "externalDatabase.host is required" .Values.externalDatabase.host -}}
postgres://{{ .Values.externalDatabase.username }}:{{ .Values.externalDatabase.password }}@{{ $host }}:{{ .Values.externalDatabase.port }}/{{ .Values.externalDatabase.database }}{{ if .Values.externalDatabase.sslMode }}?sslmode={{ .Values.externalDatabase.sslMode }}{{ end }}
{{- end -}}
{{- end }}

{{- define "liquid.backendImage" -}}
{{- printf "%s:%s" .Values.backend.image.repository (.Values.backend.image.tag | default .Chart.AppVersion) }}
{{- end }}

{{- define "liquid.frontendImage" -}}
{{- printf "%s:%s" .Values.frontend.image.repository (.Values.frontend.image.tag | default .Chart.AppVersion) }}
{{- end }}

{{- define "liquid.postgresqlImage" -}}
{{- printf "%s:%s" .Values.postgresql.image.repository .Values.postgresql.image.tag }}
{{- end }}
