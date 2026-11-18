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

{{- /* With the chart's own store, each component connects as its own Redis ACL
       user (security finding O12). Call with (dict "root" . "user" "hv2-node").
       The password comes from the Secret through HV2_STORE_PASSWORD, which every
       container using this URL must set from that role's key; Kubernetes
       expands $(VAR) in args, so it never sits in the pod spec. */ -}}
{{- define "hv2.storeUrl" -}}
{{- if .root.Values.store.deploy -}}redis://{{ .user }}:$(HV2_STORE_PASSWORD)@{{ include "hv2.name" .root }}-store:6379{{- else -}}{{ required "store.url is required when store.deploy is false" (ternary (default .root.Values.store.url .root.Values.store.nodeUrl) .root.Values.store.url (eq .user "hv2-node")) }}{{- end -}}
{{- end }}

{{- /* The store's ACL file, one user per role, from files/redis-acl-table.txt,
       which tools/redis-acl.py --check-file keeps equal to its table. The
       password hashes are placeholders (#__HASH_<ROLE>__) the store pod's init
       container fills in, so this holds no secret. */ -}}
{{- define "hv2.storeAcl" -}}
{{- $ns := toString .Values.store.namespace -}}
{{- if not (regexMatch "^[A-Za-z0-9._-]+$" $ns) -}}{{- fail "store.namespace must match [A-Za-z0-9._-]+ (a glob character would widen the store ACL)" -}}{{- end -}}
{{- $rows := list -}}
{{- $commands := dict -}}
{{- range $raw := splitList "\n" (.Files.Get "files/redis-acl-table.txt") -}}
{{- $line := trim $raw -}}
{{- $f := splitList " " $line -}}
{{- if hasPrefix "family " $line -}}{{- $rows = append $rows $f -}}
{{- else if hasPrefix "commands " $line -}}{{- $_ := set $commands (index $f 1) (join " " (slice $f 2)) -}}
{{- end -}}
{{- end -}}
{{- $roles := list (list "hv2-control-plane" 1 "CONTROL_PLANE") (list "hv2-node" 2 "NODE") (list "hv2-gateway" 3 "GATEWAY") -}}
{{- $out := list -}}
{{- range $role := $roles -}}
{{- $col := add (index $role 1) 1 | int -}}
{{- $parts := list "on" (printf "#__HASH_%s__" (index $role 2)) "resetkeys" "resetchannels" "-@all" -}}
{{- $write := false -}}
{{- range $f := $rows -}}
{{- $access := index $f $col -}}
{{- if eq $access "rw" -}}{{- $write = true -}}{{- $parts = append $parts (printf "~hv2:%s:%s" $ns (index $f 1)) -}}
{{- else if eq $access "r" -}}{{- $parts = append $parts (printf "%%R~hv2:%s:%s" $ns (index $f 1)) -}}
{{- end -}}
{{- end -}}
{{- $parts = append $parts (index $commands (ternary "rw" "r" $write)) -}}
{{- $out = append $out (printf "user %s %s" (index $role 0) (join " " $parts)) -}}
{{- end -}}
{{- $out = append $out "user default off" -}}
{{- join "\n" $out -}}
{{- end }}
