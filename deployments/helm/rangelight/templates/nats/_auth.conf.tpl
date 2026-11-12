{{- /*
The bundled NATS server's sign-in config, included by its nats.conf (ADR 0021 amendment). A named template, not
a file under files/, so the nats subchart can hash it too (the pod's checksum/auth annotation, values.yaml): a
change rolls the server. The api's tests run nats-server with this text (rl-testkit, between the define and end lines).
*/ -}}
{{- define "rangelight.natsAuthConf" -}}
# Who may connect to the bundled NATS server (ADR 0021 amendment). Included by the server's
# nats.conf; the api's tests run nats-server with this same text. $RL_NATS_ISSUER and
# $RL_NATS_AUTH_USER are public keys, from the environment (the Secret rangelight-nats-auth).

# The largest message any client may send.
max_payload: 131072

accounts {
  # The control plane and the runners, each with its own user JWT from the api's auth callout.
  RL {
    jetstream: enabled
    limits {
      max_connections: 1024
    }
    users: [
      # The api's callout connection, the only user that doesn't sign in through the callout:
      # it answers the sign-ins, asks whether a lane's terminal is already connected, and
      # takes that answer on its own inbox.
      {
        nkey: $RL_NATS_AUTH_USER
        permissions: {
          publish: {allow: ["$SYS.REQ.ACCOUNT.LANE.CONNZ"]}
          subscribe: {allow: ["$SYS.REQ.USER.AUTH", "_INBOX_callout.>"]}
          allow_responses: true
        }
      }
    ]
    # Terminal input goes to the lanes; their output and their agents' events come back (the
    # api's stream rl-<org>-agent captures the events here, where JetStream is).
    exports: [
      {stream: "rl.*.pty.*.in"}
    ]
    imports: [
      {stream: {account: LANE, subject: "rl.*.pty.*.out"}}
      {stream: {account: LANE, subject: "rl.*.agent.*.events"}}
      {service: {account: SYS, subject: "$SYS.REQ.ACCOUNT.LANE.CONNZ"}}
    ]
  }
  # The lanes, where the model runs: no JetStream, nothing but the terminal and agent events
  # subjects, and per connection one subscription and 128 KiB messages (a callout user's own
  # limits aren't applied, an account's are). Each lane's permissions narrow it to its own session.
  LANE {
    limits {
      max_connections: 1024
      max_subscriptions: 1
      max_payload: 131072
    }
    exports: [
      {stream: "rl.*.pty.*.out"}
      {stream: "rl.*.agent.*.events"}
    ]
    imports: [
      {stream: {account: RL, subject: "rl.*.pty.*.in"}}
    ]
  }
  SYS {
    exports: [
      {service: "$SYS.REQ.ACCOUNT.LANE.CONNZ"}
    ]
  }
}
system_account: SYS

authorization {
  auth_callout {
    issuer: $RL_NATS_ISSUER
    auth_users: [$RL_NATS_AUTH_USER]
    account: RL
  }
}
{{ end }}
