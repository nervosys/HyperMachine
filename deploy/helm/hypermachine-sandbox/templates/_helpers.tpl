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

{{- /* With the chart's own store, the password comes from the Secret through
       HV2_STORE_PASSWORD, which every container using this URL must set;
       Kubernetes expands $(VAR) in args, so it never sits in the pod spec. */ -}}
{{- define "hv2.storeUrl" -}}
{{- if .Values.store.deploy -}}redis://:$(HV2_STORE_PASSWORD)@{{ include "hv2.name" . }}-store:6379{{- else -}}{{ required "store.url is required when store.deploy is false" .Values.store.url }}{{- end -}}
{{- end }}
