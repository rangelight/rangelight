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

{{/* The NATS URL the api, the relay and the runner connect to. */}}
{{- define "rangelight.natsUrl" -}}
{{- if .Values.nats.enabled -}}
{{ printf "nats://%s.%s.svc:4222" .Values.nats.fullnameOverride .Release.Namespace }}
{{- else -}}
{{ .Values.nats.external.url }}
{{- end -}}
{{- end -}}

{{/* The Secret and key holding the Postgres URI, as <name>#<key>: CloudNativePG's, or postgres.external.secretRef. */}}
{{- define "rangelight.databaseRef" -}}
{{- if .Values.postgres.enabled -}}
rangelight-pg-app#uri
{{- else -}}
{{ printf "%s#%s" .Values.postgres.external.secretRef.name .Values.postgres.external.secretRef.key }}
{{- end -}}
{{- end -}}

{{/* The volume "secrets": each <name>#<key> of the list as the file <name>/<key>, where rl-config resolves
secret://<name>#<key> once mounted at /var/run/rangelight/secrets. Only those keys, never the whole Secret. */}}
{{- define "rangelight.secretsVolume" -}}
- name: "secrets"
  projected:
    sources:
      {{- range . }}
      {{- $ref := splitn "#" 2 . }}
      - secret:
          name: {{ $ref._0 | quote }}
          items:
            - key: {{ $ref._1 | quote }}
              path: {{ printf "%s/%s" $ref._0 $ref._1 | quote }}
      {{- end }}
{{- end -}}

{{/* The ingress controller's pods (ingress.from), as one NetworkPolicy peer; nothing when ingress.from is empty. */}}
{{- define "rangelight.ingressPeer" -}}
{{- with .Values.ingress.from -}}
{{- if .podLabels -}}
- podSelector:
    matchLabels:
      {{- range $key, $value := .podLabels }}
      {{ $key }}: {{ $value | quote }}
      {{- end }}
{{- else if .namespaceLabels -}}
- podSelector: {}
{{- end }}
{{- if .namespaceLabels }}
  namespaceSelector:
    matchLabels:
      {{- range $key, $value := .namespaceLabels }}
      {{ $key }}: {{ $value | quote }}
      {{- end }}
{{- end }}
{{- end -}}
{{- end -}}

{{/* Egress to the cluster DNS (clusterDns): the only DNS server a Rangelight pod may query. */}}
{{- define "rangelight.dnsEgress" -}}
{{- $dns := splitn "=" 2 .Values.clusterDns.podLabel -}}
- to:
    - namespaceSelector:
        matchLabels:
          kubernetes.io/metadata.name: {{ .Values.clusterDns.namespace | quote }}
      podSelector:
        matchLabels:
          {{ $dns._0 }}: {{ $dns._1 | quote }}
  ports:
    - protocol: "UDP"
      port: 53
    - protocol: "TCP"
      port: 53
{{- end -}}

{{/* Egress to Postgres and NATS: the bundled pods, or, external, their port anywhere (the chart can't know the
host). */}}
{{- define "rangelight.storeEgress" -}}
{{- if .Values.postgres.enabled -}}
- to:
    - podSelector:
        matchLabels:
          cnpg.io/cluster: "rangelight-pg"
  ports:
    - protocol: "TCP"
      port: 5432
{{- else -}}
- ports:
    - protocol: "TCP"
      port: 5432
{{- end }}
{{ if .Values.nats.enabled -}}
- to:
    - podSelector:
        matchLabels:
          app.kubernetes.io/name: "nats"
          app.kubernetes.io/instance: {{ .Release.Name | quote }}
  ports:
    - protocol: "TCP"
      port: 4222
{{- else -}}
- ports:
    - protocol: "TCP"
      port: {{ regexFind "[0-9]+$" (regexFind ":[0-9]+$" .Values.nats.external.url) | default "4222" | int }}
{{- end -}}
{{- end -}}

{{/* Egress to HTTPS outside the cluster (clusterCidrs): Linear, GitHub, an OIDC provider; never a pod or
Service. */}}
{{- define "rangelight.httpsEgress" -}}
- to:
    - ipBlock:
        cidr: "0.0.0.0/0"
        except:
          {{- range .Values.clusterCidrs }}
          - {{ . | quote }}
          {{- end }}
  ports:
    - protocol: "TCP"
      port: 443
    {{- /* Your own issuer on another port, for example Keycloak's :8443. */}}
    {{- $port := regexFind "^[^/]+:[0-9]+" (trimPrefix "https://" .Values.oidc.issuer) | splitList ":" | last }}
    {{- if and (not .Values.dex.enabled) $port (ne $port "443") }}
    - protocol: "TCP"
      port: {{ $port | int }}
    {{- end }}
{{- end -}}

{{/* A container's resources from a values block: CPU and memory requests, a memory limit and an optional CPU limit. */}}
{{- define "rangelight.resources" -}}
resources:
  requests:
    cpu: {{ .requests.cpu | quote }}
    memory: {{ .requests.memory | quote }}
  limits:
    {{- with .limits.cpu }}
    cpu: {{ . | quote }}
    {{- end }}
    memory: {{ .limits.memory | quote }}
{{- end -}}

{{/* The api's NATS seeds as <name>#<key>, one per line: the callout issuer's and the auth user's, from the keys Job's
Secret or nats.auth.existingSecret, and in operator mode (nats.auth.laneAccount is a public key) LANE's signing key. */}}
{{- define "rangelight.natsSeeds" -}}
{{- $secret := .Values.nats.auth.existingSecret | default "rangelight-api-nats" -}}
{{ $secret }}#issuer-seed
{{ $secret }}#auth-user-seed
{{- if ne .Values.nats.auth.laneAccount "LANE" }}
{{ $secret }}#lane-issuer-seed
{{- end }}
{{- end -}}

{{/* The volume "nats-token": the workload's projected ServiceAccount token for NATS (audience rangelight-nats), which
the api's auth callout maps to its identity. The kubelet renews it before its hour is up; clients read it at each
connect. Mounted at /var/run/secrets/rangelight/nats. */}}
{{- define "rangelight.natsTokenVolume" -}}
- name: "nats-token"
  projected:
    sources:
      - serviceAccountToken:
          audience: "rangelight-nats"
          expirationSeconds: 3600
          path: "token"
{{- end -}}
