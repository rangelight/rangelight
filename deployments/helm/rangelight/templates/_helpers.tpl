{{/* The labels every Rangelight object carries. Call with (dict "ctx" $ "name" "<name>" "component" "<component>"). */}}
{{- define "rangelight.labels" -}}
{{ include "rangelight.selector" . }}
app.kubernetes.io/component: {{ .component | quote }}
app.kubernetes.io/part-of: "rangelight"
app.kubernetes.io/managed-by: {{ .ctx.Release.Service | quote }}
helm.sh/chart: {{ printf "%s-%s" .ctx.Chart.Name .ctx.Chart.Version | quote }}
{{- end -}}

{{/* The labels a selector matches on: the component's name and the release. */}}
{{- define "rangelight.selector" -}}
app.kubernetes.io/name: {{ .name | quote }}
app.kubernetes.io/instance: {{ .ctx.Release.Name | quote }}
{{- end -}}
