// The Code panel: a file of a sample lane's worktree.
const FILE = {
  repo: "storefront",
  path: "src/refund.rs",
  lines: [
    "/// Refunds only what didn't land; safe to call twice.",
    "pub fn refund_unlanded(&self, req: RequestId, key: IdemKey) -> Result<Refund> {",
    "    if let Some(done) = self.idem.get(&key)? {",
    "        return Ok(done);",
    "    }",
    "    let held = self.hold(req)?;",
    "    let landed = self.landed(req)?;",
    "    self.write(Refund::new(req, round_half_even(held - landed)), key)",
    "}",
  ],
};

export function useCode(key: string) {
  return { file: key ? FILE : undefined };
}
