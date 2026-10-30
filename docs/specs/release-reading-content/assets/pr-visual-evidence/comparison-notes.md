# Feed Recovery Visual Comparison

- Baseline: [production failure screenshot](./dashboard-readable-fallback-production.png)
- Candidate: [mock Dashboard recovery state](../dashboard-readable-fallback-desktop.png)
- Difference image: [reg-cli comparison output](./dashboard-readable-fallback-diff.png)
- Comparison pair: `feed-recovery-dashboard`, 1206 x 1200 px

The production screenshot records the unreadable Compare failure. The mock Dashboard shows the original Release content remaining readable in the fallback state. The mock does not reproduce the production Inbox layout, so this evidence supports the recovery behavior and does not claim layout parity.
