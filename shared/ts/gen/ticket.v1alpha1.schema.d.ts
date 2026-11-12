// Generated from api/jsonschema/ by scripts/check.sh gen. Do not edit.

/**
 * The ticket a session works on: the lane's brief and its acceptance criteria (ADR 0045 §3). The runner mounts it in the lane as /etc/rangelight/ticket.json.
 */
export interface RangelightTicketV1Alpha1 {
  /**
   * The tracker's key, for example RAN-35. The lane namespace is rl-<key, lowercased>, so it stays a DNS label.
   */
  key: string;
  title: string;
  description: string;
  /**
   * The acceptance criteria, AC1..ACn in order.
   *
   * @minItems 1
   * @maxItems 50
   */
  criteria: [string, ...string[]];
  /**
   * The ticket's page in Linear, which the session's pull request links. The runner keeps it out of the lane's ticket.json.
   */
  url?: string;
}
