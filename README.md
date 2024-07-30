# gemini-3h-slash
Script to retrieve slashed balance for Operator's Nominators

## Fetching slashed operators
Use [Squid](https://squid.gemini-3h.subspace.network/graphql) with the following query
```
query MyQuery {
  events(limit: 100, where: {name_containsInsensitive: "OperatorSlashed", timestamp_gte: "2024-07-01T00:00:00Z"}) {
    args
    block {
      height
    }
  }
}

```
Timestamp can be adjusted if needed but for Gemini-3h, slash for Invalid Bundles started after the July 1st.
The current list in the script already contains all the slashed operators but do check another time before running the script to ensure there are no new operators slashed due to InvalidBundle issue.

The response for this query would be something as follows. You just need to capture the `operatorId` and `block.height`
```json
{
  "data": {
    "events": [
      {
        "args": {
          "reason": {
            "value": 1134397,
            "__kind": "InvalidBundle"
          },
          "operatorId": "65"
        },
        "block": {
          "height": "2364057"
        }
      }
    ]
  }
}
```
Then update `get_slashed_operators` function accordingly.

## Transferring the slashed balance from Treasury

To run the script, you would need to have `Sudo` key accessible and script can be run as follows:
`cargo run -- --keystore-suri "//Alice"`

The script does following:
- For each operator, fetches all nominators
- For each nominator, calculates their stake and bundle storage fee a block before the operator was slashed.
- For each operator, creates `Utility.batch_all` with all the nominators and their slashed balance to be transferred from treasury.

Note:
Script does ensure Treasury account has enough balance before dispatching the calls.
In case, if a batch fails for a given operator, you would need to adjust the `get_slashed_operators` to include only those operators for which batch failed.

Since this is a one of script, I did not include handling above failed scenario since that would require some form of storage layer. If this is used in future again, I recommend handling this. 