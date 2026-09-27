{{- define "hv2.name" -}}{{ .Release.Name }}-hv2{{- end }}

{{- define "hv2.labels" -}}
app.kubernetes.io/part-of: hypermachine-sandbox
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
helm.sh/chart: {{ .Chart.Name }}-{{ .Chart.Version }}
{{- end }}

{{- define "hv2.secretName" -}}
{{- if .Values.auth.existingSecret -}}{{ .Values.auth.existingSecret }}{{- else -}}{{ include "hv2.name" . }}-auth{{- end -}}
{{- end }}

{{- define "hv2.storeUrl" -}}
{{- if .Values.store.deploy -}}redis://{{ include "hv2.name" . }}-store:6379{{- else -}}{{ required "store.url is required when store.deploy is false" .Values.store.url }}{{- end -}}
{{- end }}
