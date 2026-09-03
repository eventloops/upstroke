# Pagination rework

## Design the pagination API
<!-- upstroke: id=api-design kind=design depends= tier=frontier out=api-contract -->
Define cursor format, page-size limits, and error contract.

Acceptance:
- Cursor format documented
- Error contract covers empty pages

## Implement cursor encoding
<!-- upstroke: id=cursors kind=implement depends=api-design needs=api-contract paths=src/api/** -->
Implement opaque cursor encode/decode per the contract.

## Fix off-by-one in list endpoint
<!-- upstroke: id=fix-obo kind=fix depends=cursors min=mid paths=src/api/** -->

## Update API docs
<!-- upstroke: id=docs kind=docs depends=fix-obo -->
