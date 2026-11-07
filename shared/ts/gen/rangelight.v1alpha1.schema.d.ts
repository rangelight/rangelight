// Generated from api/jsonschema/ by scripts/check.sh gen. Do not edit.

/**
 * RFC 1123 label, at most 50 characters (ADR 0019 slug rule).
 *
 * This interface was referenced by `RangelightYamlV1Alpha1PreviewProfile`'s JSON-Schema
 * via the `definition` "dnsLabel".
 */
export type DnsLabel = string;

/**
 * A thin preview profile: references to the org's existing Argo source plus what Rangelight needs to run a ticket's copy of prod. It is not a deployment format.
 */
export interface RangelightYamlV1Alpha1PreviewProfile {
  apiVersion: "rangelight.dev/v1alpha1";
  kind: "PreviewProfile";
  metadata: {
    name: DnsLabel;
    /**
     * Human who reviewed the agent-derived profile.
     */
    pinnedBy?: string;
  };
  spec: {
    /**
     * Path to devcontainer.json in this repo (ADR 0013). Defaults to .devcontainer/devcontainer.json.
     */
    devcontainer?: string;
    /**
     * Rendered into one Argo Application per session with automated sync (ADRs 0019, 0014).
     */
    argo?: {
      source: {
        [k: string]: unknown;
      } & {
        repoURL: string;
        chart?: string;
        path?: string;
        targetRevision?: string;
        helm?: {
          valueFiles?: string[];
          values?: {
            [k: string]: unknown;
          };
        };
      };
      /**
       * Preview-only changes on top of the org's source: one replica, small resources, disabled extras.
       */
      overlay?: {
        valueFiles?: string[];
        values?: {
          [k: string]: unknown;
        };
      };
    };
    /**
     * @minItems 1
     */
    services: [
      {
        name: DnsLabel;
        /**
         * Kubernetes Service name neighbours call.
         */
        serviceName: string;
        port: number;
        /**
         * Publish at <name>--<session>.preview.<domain> behind forward-auth (ADR 0039).
         */
        expose?: boolean;
        /**
         * Present when this service may run hot-reloaded in the agent pod (ADR 0014).
         */
        underChange?: {
          repoPath: string;
          /**
           * Hot-reload command run in the agent pod.
           */
          command: string;
          env?: {
            [k: string]: string;
          };
          /**
           * Argo values that remove the org's workload for this service; Rangelight then serves a Service of the same name selecting the agent pod.
           */
          park: {
            values: {
              [k: string]: unknown;
            };
          };
        };
      },
      ...{
        name: DnsLabel;
        /**
         * Kubernetes Service name neighbours call.
         */
        serviceName: string;
        port: number;
        /**
         * Publish at <name>--<session>.preview.<domain> behind forward-auth (ADR 0039).
         */
        expose?: boolean;
        /**
         * Present when this service may run hot-reloaded in the agent pod (ADR 0014).
         */
        underChange?: {
          repoPath: string;
          /**
           * Hot-reload command run in the agent pod.
           */
          command: string;
          env?: {
            [k: string]: string;
          };
          /**
           * Argo values that remove the org's workload for this service; Rangelight then serves a Service of the same name selecting the agent pod.
           */
          park: {
            values: {
              [k: string]: unknown;
            };
          };
        };
      }[]
    ];
    data?: {
      name: string;
      engine: "postgres" | "mysql" | "valkey" | "redis" | "mongodb" | "kafka" | "other";
      mode: "fixtures" | "masked-snapshot";
      /**
       * Command that applies the repo's migrations.
       */
      migrate?: string;
      /**
       * fixtures: command that loads the repo's seed data.
       */
      seed?: string;
      /**
       * masked-snapshot: ref into rangelight-config, e.g. config://masking/<name>.yaml.
       */
      maskingRules?: string;
      /**
       * masked-snapshot: credential-ref of the staging/prod source, used only by the trusted golden-copy job.
       */
      goldenSource?: string;
    }[];
    externalDeps?: {
      name: string;
      mode: "mock" | "sandbox" | "block";
      hosts?: string[];
      mock?: {
        engine: "prism" | "wiremock";
        spec: string;
      };
      /**
       * sandbox: credential-ref from the preview secret set.
       */
      sandboxCredential?: string;
    }[];
    secrets?: {
      /**
       * Name of the preview secret set in rangelight-config (ADR 0038). Prod secrets are never allowed.
       */
      previewSet: string;
    };
    /**
     * Cluster-scoped or operator-managed resources that must already exist; Rangelight never renders them per session.
     */
    shared?: {
      apiVersion?: string;
      kind: string;
      name: string;
    }[];
    /**
     * Extra gates on top of the built-in layers (ADR 0024).
     */
    readiness?: {
      layer: "deps" | "neighbours" | "schema" | "service" | "preview";
      check: {
        /**
         * URL inside the namespace that must return 2xx.
         */
        http?: string;
        /**
         * host:port that must accept connections.
         */
        tcp?: string;
        command?: string;
      };
      timeoutSeconds?: number;
    }[];
    /**
     * The test command Rangelight runs itself in the observed gate Job (ADR 0045 §3): on the pushed head, on main, and on main merged with the branch. Tests whose names contain AC<n> prove acceptance criterion n.
     */
    tests?: {
      /**
       * Shell command run from repoPath, e.g. 'npm test -- --test-reporter=tap'.
       */
      command: string;
      /**
       * How Rangelight reads results: TAP on stdout, or JUnit XML at reportPath.
       */
      format: "tap" | "junit";
      /**
       * JUnit XML path, relative to repoPath. Required when format is junit.
       */
      reportPath?: string;
      repoPath?: string;
      timeoutSeconds?: number;
    };
    /**
     * Verify any branch (ADR 0048).
     */
    verify?: {
      /**
       * Verify same-repo pull requests automatically.
       */
      pullRequests?: boolean;
      maxConcurrent?: number;
    };
    /**
     * Check Runs from the org's own CI that a lane's pushed head must also pass (ADR 0054 §1). A Check Run from any app other than `app` is ignored.
     */
    externalChecks?: {
      /**
       * The Check Run name, e.g. 'Jenkins' or 'ci / test'.
       */
      name: string;
      /**
       * Slug of the only GitHub App allowed to publish this check, e.g. 'github-actions'.
       */
      app: string;
      required?: boolean;
    }[];
  };
}
