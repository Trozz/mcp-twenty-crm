# CRM

Endpoint `/crm` (or `crm_<tool>` under [`/mcp`](combined.md)). The CRM service reads and changes one Twenty workspace through Twenty's REST API, with the signed-in person's own API key.

## Objects

Records belong to objects, named by their plural API name: `people`, `companies`, `opportunities`, `notes`, `tasks`, `workspaceMembers` and any custom objects. `list_objects` lists them and `describe_object` gives an object's fields, their types, select values and relations. Records and ids come from the search and list tools; ids are UUIDs.

## Tools

| Tool | Kind | What it does | Key parameters |
|---|---|---|---|
| `search_records` | read | Find records by name across objects (people also by email, notes and tasks by title, custom objects by name). Every word of the query must match. Returns up to `limit` records per object | `query` (up to 5 words), `objects` (default people, companies, opportunities; up to 8), `limit` (1 to 50, default 5) |
| `list_records` | read | Page through any object with an optional filter and sort. Returns `total`, `count`, `items` and `next_cursor` while there are more pages | `object`, `filter`, `order_by`, `limit` (1 to 200, default 20), `cursor`, `fields`, `depth` (0 or 1, default 0) |
| `get_record` | read | Read one record by id, with its related records in full unless `depth` is 0 | `object`, `id`, `depth` (0 or 1, default 1) |
| `list_objects` | read | The workspace's objects: plural API name, singular name, label and description, and whether an object is custom | `include_system` (default false) |
| `describe_object` | read | One object's fields: API name, type, label, whether it is required, the values a `SELECT` or `MULTI_SELECT` accepts, and for a `RELATION` its kind and `set_with`, the field to set with the related record's id (such as `companyId`) | `object` (plural or singular name), `include_system` (default false) |
| `list_workspace_members` | read | The people who use Twenty, with their ids for assigning tasks and owning accounts | `limit` (1 to 200, default 20), `cursor` |
| `list_record_activity` | read | The notes and tasks linked to one person, company or opportunity, newest first, with their Markdown bodies, due dates and status | `object` (people, companies or opportunities), `id`, `limit` (1 to 200 each, default 20) |
| `create_person` | write | Create a person from plain fields; the composite fields are built for you | `first_name`, `last_name`, `email`, `phone`, `phone_calling_code`, `job_title`, `city`, `company_id`, `linkedin_url`, `extra` |
| `create_company` | write | Create a company from plain fields | `name`, `domain`, `employees`, `linkedin_url`, `address_street`, `city`, `postcode`, `country`, `account_owner_id`, `extra` |
| `create_opportunity` | write | Create an opportunity (deal) from plain fields | `name`, `amount`, `currency_code`, `stage`, `close_date`, `company_id`, `point_of_contact_id`, `extra` |
| `create_note` | write | Create a note with a Markdown body and link it to people, companies and opportunities in one call | `title`, `body`, `person_ids`, `company_ids`, `opportunity_ids` |
| `create_task` | write | Create a task with an optional due date, status, assignee and Markdown body, linked to people, companies and opportunities | `title`, `body`, `due_at`, `status` (`TODO`, `IN_PROGRESS` or `DONE`), `assignee_id`, `person_ids`, `company_ids`, `opportunity_ids` |
| `create_record` | write | Create one record of any object from its own fields | `object`, `data` |
| `create_records` | write | Create 1 to 60 records of one object in a single request, for example to import a list | `object`, `records` |
| `update_record` | write | Change fields of one record of any object. Only the fields given change | `object`, `id`, `data` |
| `delete_record` | write | Move a record to the trash (the default), or destroy it with `permanent: true` | `object`, `id`, `permanent` (default false) |
| `restore_record` | write | Bring a record back from the trash | `object`, `id` |

## Prompts

Prompts are ready-made requests a client can offer you. They tell the model which tools to call and in what order; they change nothing by themselves.

| Prompt | Arguments | What it does |
|---|---|---|
| `account_brief` | `company` (required) | A briefing on one company: finds it with `search_records`, reads the record, the people there, its opportunities and its notes and tasks, then writes a summary with company details, people, opportunities, recent activity and next steps. If more than one company could match, it asks which |
| `log_interaction` | `summary` (required) | Turns a summary of a call, meeting or email into a note and follow-up tasks linked to the right people, companies and opportunities. It shows you what it will create and changes nothing until you confirm |

Arguments are text of up to 4,000 characters. Both prompts tell the model to treat record text as data, not instructions, and how to react to each tool error kind. On a read-only CRM service the prompts are still offered; a step that needs a hidden tool tells the model to say so and skip it.

## Filters and sorting

`list_records` passes `filter` and `order_by` to Twenty as they are, so Twenty's own syntax applies:

- A filter is `field[comparator]:value`, for example `name[ilike]:%acme%` or `companyId[eq]:<id>`.
- Comparators: `eq`, `neq`, `in`, `containsAny`, `is` (`NULL` or `NOT_NULL`), `gt`, `gte`, `lt`, `lte`, `startsWith`, `like`, `ilike`.
- Combine with `and(...)`, `or(...)` and `not(...)`, for example `and(stage[eq]:NEW,amount.amountMicros[gte]:1000000000)`.
- Composite subfields use dots: `emails.primaryEmail`, `name.lastName`, `amount.amountMicros`.
- `order_by` is field names with an optional direction, separated by commas: `createdAt[DescNullsLast]` or `name.firstName,createdAt[AscNullsFirst]`. Directions are `AscNullsFirst`, `AscNullsLast`, `DescNullsFirst` and `DescNullsLast`.
- `fields` limits the fields returned (up to 100 names; `id` is always included).
- Pass `next_cursor` from one page as `cursor` to get the next.

Filters are limited to 2,000 characters.

## Composite fields

Twenty stores some fields as objects. `create_record` and `update_record` take them in Twenty's own shape:

| Field | Shape |
|---|---|
| `name` (people) | `{firstName, lastName}` |
| `emails` | `{primaryEmail, additionalEmails}` |
| `phones` | `{primaryPhoneNumber, primaryPhoneCallingCode}` |
| `domainName`, `linkedinLink` | `{primaryLinkUrl}` |
| `address` | `{addressStreet1, addressCity, addressPostcode, addressCountry}` |
| `amount` | `{amountMicros, currencyCode}`, where `amountMicros` is the amount times 1,000,000 |

Relations take the related record's id, for example `{"companyId": "<id>"}`. In `update_record`, a composite field is replaced as a whole, so give every part you want to keep. `data` must hold at least one field and must not contain `id`.

The `create_person`, `create_company` and `create_opportunity` tools take plain fields and build these shapes for you. Their `extra` parameter takes any other field by API name, including custom ones; the plain fields win over `extra`, which must not contain `id`.

- `create_opportunity` turns `amount` (for example `12500.50`) into micros, with `currency_code` or else `TWENTY_DEFAULT_CURRENCY`. Amounts must be below 1,000,000,000,000. `stage` is upper-cased with spaces and hyphens turned into `_`, so `proposal` becomes `PROPOSAL`; `describe_object opportunities` lists the stages your workspace has.
- `close_date` and `due_at` are ISO 8601 date-times, for example `2026-06-30T00:00:00Z`.

## Notes and tasks

- Bodies are Markdown, up to 100,000 characters, stored in Twenty's `bodyV2` field. Outputs show the Markdown without the editor's block JSON.
- Links are made through `noteTargets` and `taskTargets` with the `targetPerson`, `targetCompany` and `targetOpportunity` fields, one request per link, up to 60 ids per list.
- If some links fail, the note or task is still created: the result lists `link_errors` and says not to create it again. Link the rest with `create_record` on `noteTargets` or `taskTargets`.

## Deleting

Twenty's own DELETE destroys a record unless told otherwise. `delete_record` always asks for a soft delete unless `permanent` is `true`, so by default the record goes to the trash and `restore_record` brings it back.

## Output

Results are compact JSON: Twenty's envelopes are unwrapped, empty values are dropped, and `searchVector` and `avatarUrl` (which carries a signed file token) are removed. Pages carry `object`, `total` when Twenty gives it, `count`, `items` and `next_cursor` while there are more.

## Twenty notes

- Twenty allows about 100 API requests per minute per workspace. When the limit is hit, tools fail with `rate_limited` and say how long to wait, using Twenty's `Retry-After` when it sends one.
- Reads are retried once after a network error or a server error; writes never are, so a slow create cannot run twice.
- Requests time out after 30 seconds (10 seconds to connect), and responses larger than 16 MiB are refused; ask for fewer records or fields.
- Listing and describing objects needs a key whose role may read the data model.
- Free-text arguments such as names and titles are limited to 500 characters.
