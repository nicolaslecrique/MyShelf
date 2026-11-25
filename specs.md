## Myshelf

## User specifications

* You create your account on the web
* Then you can connect your "agent" to it (claude, chatgpt...), then you can use it from your claude / chatgpt app on mobile too
* The user could also vibe-code a app to display and manage a specific shelf.
* it allows you to define categories of item you can store (called shelves), like "movies", "todo", "things to buy", "personal CRM", "business ideas", "science paper i should watch", and find them later, by filtering on some attributes (dates, number, search in a text field...)
* There is not web frontend outsided login, everything is mediated through an agent and the provided MCP server, wrapping a Rest API (also exposed to clients)
* A shelf follows a specific json schema

### Concepts:

* Shelf: A collection of items sharing the same structure. Ex: Movies
* Schema: The structure shared by the items of a shelf; Ex: title, watched, category.
* Item: An individual record. ex: Love Actually


### User story 1: Create a shelf, update it, add an item

* The user asks to store a new kind of info: Movies I watched or I want to watch
* The agent suggest a list of properties about the movie: 

* User: Store a new kind of info on MyShelf: the movies I want to watch
* Agent: Ok, Which info should i store for each movie so you can find one you stored easily? I propose the title, the link to IMDB, and the movie category ?
* User: yes, and add the person that recommended it to me
* Agent: Ok, I created a new Shelf "Movies to watch", here is the infos i saved: "title", "IMDB url", "category", "recommended by". Should I add a first movie ?
* User: actually, store all the movies, even if i have already seen them
* Agent: I added a new info "watched:yes/no", and changed the name of the shelf to "Movies".
* Add love actually
* I'll search the relevant infos...
* I found the IMDB url, should I add someone that recommended it to you ? And did you already watch it
* Yes, nobody recommended it
* Done.

### User story 2: search for stored items.

* user: what are the movies I watch last year where actor XXX played
* Agent: I will load all the movies you marked as watched last year, and the list of movies where XXX played from internet, then compare...


## Non functional requirements

- The service should be self-hostable
- The service is provided as a SAAS
- The service is free for the vast majority of users, so it should be as low-cost as possible to operate, even when it scales
- The user's data should be as safe (from a privacy point of view) as possible. Both in practice and from the user perception

### Encryption

We use envelop encryption: 

- A master key, stored in a KMS
- A KEK per user, encrypted by the master key, stored encrypted in app db
- A DEK by Shelf, protecting both the shelf definition and the items of that shelf
- Each dek shelf key is encrypted by the user KEK key, and stored in a user+ shelf join table.
- on a user request, we decrypt the user key, then decrypt the shelves keyes with the user key
- for rotation, besides user KEK, we keep the KMS key version id, an ine shelf-user table, we keep the KEK version id
- the search (for a given user and a given shelf) is executed in-memory

### Storage

- Everything in pg
- list of tables:
    - user, id, metadata
    - user_shelf, encrypted_shelf_key, user_id, shelf_id
    - shelf: id, encrypted_title, encrypted_description, encrypted_jsonschema, metadata
    - item: id, encrypted_title, encrypted_json_content, metadata, shelf_id

- when we store an item, we serialize with MessagePack, compress with Zstd, encrypt with the shelf key, then store it
- json should be less than 1MB (at reception, before any optimization)
- metadata is created_at, updated_at, archived_at


### List of operations

#### Shelf

- create a shelf (title, description, json schema) -> id + all data
- modify a shelf (id, title, description, json schema) -> id + all data
- archive a shelf (id) -> ack. prevent any other operation on it
- unarchive shelf (id) -> ack
- list shelves -> list[id, title, description, metadata]
- list archived shelves -> list[id, title, description, metadata]
- get shelf (id) ->  id + all data

#### item

- create item (shelf_id, title, json_content) -> id, all data
- modify item (id, title, json_content) -> id, all data
- archive item (id) -> ack. prevent any other operation on it
- unarchive item (id) -> ack
- search items (shelf_id, search_query, max_results, with_content) -> sorted list[id, title, description, data (if requested), metadata], paginated. pagination size depends on payload size.
- list archived items (shelf_id) -> list[id, title, description], paginated.
- get item (shelf_id, id) -> all item data


### Search

We load only title / description of all items of the shelf, decrypt them, then do a basic search (contains).


### Schema migration

For now, when we modify a schema, we check that all items of the shelf are still consistent with the new schema.

### Authentication

Identity Provider: Generic OIDC, using WorkOS initially.
Authentication: oauth-resource-server for JWT validation (JWKS, issuer, audience, expiration).
OAuth flow: Authorization Code + PKCE, handled by the MCP client and WorkOS.
Authorization: No granular permissions or scopes required. Users have full access to their own data only.
Data isolation: Every resource is associated with its owner's user ID (sub from the JWT). All database operations enforce ownership.
Architecture: Shared authentication and business logic between REST and MCP.

## Tech stack

Language: Rust
Async runtime: Tokio
HTTP framework: Axum
MCP: rmcp
Serialization: Serde
JSON storage: encrypted version of json as text, then MessagedPack (rmp_serde) and/or Zstd compression if measured relevant by benchmarks
JSON: serde_json
JSONPath: jsonpath-rust
Encryption: RustCrypto aes-gcm
PostgreSQL	SQLx
Migrations	SQLx migrations
Authentication lib: oauth-resource-server
OICD identity provider: cloud logto
Monitoring.



## Out of Scope for MVP

- Ability to Store larger file in S3
- caching DEK keys if KMS is becoming too expensive
- manage relationship and links between shelves
- sharing a shelf between several people
- workspace / teams
- key rotation, we just keep key versions to prepare for it
- json migration when schema changes (probably using Jsonata), for now, the new jsonschema should be compatible with the current jsons.
- get filtered list of shelf (search shelves)
- ability to filter items in search using json query (probably jsonpath)
- ability to filter items in search within title, description or json content (tantivy, rapidfuzz)
- additional isolation using Row Level Security
- using blind index for more efficient search in DB
- using things like rockDB for more optimal get/update of data

## Accepted trade-offs

- using envelop encryption of all data makes search happens in-memory, without db index, this put pressure on the amount of data loaded from the db. This is accepted for maximum security
- using server-side encryption rather than e2ee makes it theoretically possible to leak user data. This is accepted to allow for efficient server-side filtering and cloud-based MCP server usable from llm providers mobile apps.


## Dev env

* strict linting:

    [lints.rust]
    unsafe_code = "deny"

    [lints.clippy]
    pedantic = { level = "deny", priority = -1 }

* cargo fmt

## Hosting

* deployed in Scaleway Serverless Containers
* use managed postgres

## Architecture

inspired by hexagonal architecture.

- data layer: only module to call the db, db and sql is not referenced elsewhere, except a opaque ref to manage transactions between several calls to the data layer
- core: business logic, called by mcp and rest
- search engine: manage search
- encryption: manage encryption
- mcp
- rest api: routes, api structures and mapping to core
- figment for managing configuration


### Tests

when possible, most features are tested end to end, as a user would call the service, against a postgres db in a docker container