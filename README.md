# Planner

A project planning tool.

## Next steps for v1
*  Police invariants, and rely on them
   *  Only leaf nodes can have done data
   *  All leaf nodes have plan/dev data
   *  A dev's data is always accessible (ie all devs are valid devs)
*  Calculate plan figures during display.  Propagate up to budget heads.
*  Display slip/gain against budget heads
*  Per-person view
*  Expand/contract node trees
*  Formatting of top-level nodes
*  Re-implement serial resourcing
 *  Idea - descend the nodes flagging which to update, then process.

## Later display options
*  Slip/gain from last week
*  Personal daily spreadsheet
*  Display "from-now", omitting completed tasks
*  Display individual PRDs
*  Display budgets only
*  Historical display - budget and planned numbers changing over time

## Minor tidy-ups
*  Sort out formatting on multi-line notes
*  Order the developer rows to match plan
*  Text alignment of week labels above labels should be centered, not left-justified.