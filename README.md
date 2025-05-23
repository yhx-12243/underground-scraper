# Underground Forum Scraper Collection

## Introduction & Description

These project contain a collection of scraper of common underground forums as well as something like [Telegram](https://telegram.org/) and so on. The whole project is written in [Rust](https://www.rust-lang.org/) (mainly), which provides a stable and efficient environment to work with.

## Table of Contents

* [Requirements](#requirements)
* [Environment](#environment)
  - [Global Environment Variables](#global-environment-variables)
  - [Patches](#patches)
* [Scrapers](#scrapers)
  - [AccsMarket](#accsmarket)
  - [EZKIFY Services](#ezkify-services)
  - [BlackHatWorld](#blackhatworld)
  - [Telegram](#telegram)

## Requirements

The project has following requirements:

* A Rust toolchain in a `nightly` version (not too old, 1.75+ is sufficient),
* A [PostgreSQL](https://www.postgresql.org/) client (and server, if you don't have, v14+ is OK).
* (Optional) A Python 3 running environment (3.10+ is OK).

The other dependencies will download when building by *Cargo*, please keep the network connection well.

## Environment

### Global Environment Variables

Since we use PostgreSQL for our data storage and management, and all the program requires the following environment of PostgreSQL, please **set/change them based your actual condition**:

```sh
export DB_HOST=/var/run/postgresql
export DB_USER=postgres
export DB_NAME=postgres
export DB_PASSWORD=<password> # optional
```

**Note**: We use this environment **in `cargo build` process**.

Besides, we use [`log`](https://crates.io/crates/log) and [`tracing`](https://crates.io/crates/tracing) for debugging and logging, so it's better to turn the `RUST_LOG` on:

```sh
export RUST_LOG=info
```

### Patches

We have some patch files for some Rust third-party libraries, they lie in [`./patches/*.patch`](./patches) directory, **you should apply them before compiling**.

If you don't know how to apply them, you can just run the script [`./apply_patch.py`](./apply_patch.py) **before `cargo build`** to finish (although it may not be so robust but it usually works fine).

In addition, all the patch we write are mild (compatible), namely, any program can pass without patch will always pass with patch and produce the same result. For example, we only make some private interface public, or add some interfaces.

## Scrapers

Currently our scraper collection contains five programs: AccsMarket, EZKIFY Services, BlackHatWorld and Telegram. Each part has a independent *Schema* in (PostgreSQL) database, and the correspondent Schema will be described below.

Plus, we will briefly describe the methodology of each scraper, in order to help users to read the code when accidentally run into bugs.

---

First, you should run `cargo build [-r]` ([`-r` means release mode](https://doc.rust-lang.org/cargo/commands/cargo-build.html#option-cargo-build--r)) to build all of the binaries.

If your build fails with errors, it's likely that you skipped [the step of patch applying](#patches) or did not perform it correctly, please **run `cargo clean` first**, then check it out again.

When the build succeeds, you can run `cargo run -r --bin <name> -- <args>` to start these scrapers. For convenience, we use `./foo <args>` to simply denote `cargo run -r --bin foo -- <args>` (of course you can copy the binary into working directory).

---

[AccsMarket](#accsmarket) and [EZKIFY Services](#ezkify-services) have a relatively weak defense system, so we just use `reqwest` to interchange packets and use `scraper` to parse data. It is completely one-click.

[BlackHatWorld](#blackhatworld) has a stronger defense system involving [Cloudflare](https://www.cloudflare.com/), so we use the [ChromeDriver](https://chromedriver.chromium.org/) technique, assisting manual verification to scrape data efficiently.

[Telegram](#telegram) is a multifunctional CLI program which integrates many way to scrape channels/messages and analyze data. It uses the [Telegram API](https://core.telegram.org/) to deal with and work.

### AccsMarket

#### SQL Schema

```sql
DROP SCHEMA IF EXISTS accs CASCADE;

CREATE SCHEMA accs;

CREATE TABLE accs.market (
    id bigint NOT NULL,
    category bigint NOT NULL,
    "time" timestamp without time zone NOT NULL,
    description text NOT NULL,
    quantity bigint NOT NULL,
    price double precision NOT NULL,
    PRIMARY KEY (id, "time")
);
```

#### Usage

```sh
./accsmarket
```

It will automatically scrape all the data from https://accsmarket.com/ into the created database, the whole process takes about 40 ~ 60 secs.

### EZKIFY Services

#### SQL Schema

```sql
DROP SCHEMA IF EXISTS ezkify CASCADE;

CREATE SCHEMA ezkify;

CREATE TABLE ezkify.categories (
    key text NOT NULL,
    id bigint NOT NULL,
    "desc" text NOT NULL,
    PRIMARY KEY (key, id),
);

CREATE TABLE ezkify.items (
    key text NOT NULL,
    id bigint NOT NULL,
    "time" timestamp without time zone NOT NULL,
    category_id bigint NOT NULL,
    service text NOT NULL,
    rate_per_1k double precision NOT NULL,
    min_order bigint NOT NULL,
    max_order bigint NOT NULL,
    description text NOT NULL,
    PRIMARY KEY (key, id, "time"),
    FOREIGN KEY (key, category_id) REFERENCES ezkify.categories(key, id)
);
```

#### Usage

```sh
./ezkify
```

It will automatically scrape all the data from https://ezkify.com/services into the created database, the whole process takes about 10 ~ 20 secs.

### BlackHatWorld

#### SQL Schema

```sql
DROP SCHEMA IF EXISTS blackhatworld CASCADE;

CREATE SCHEMA blackhatworld;

CREATE TABLE blackhatworld.content (
    id bigint NOT NULL,
    content text NOT NULL,
    PRIMARY KEY (id)
);

CREATE TABLE blackhatworld.posts (
    id bigint NOT NULL,
    "time" timestamp without time zone NOT NULL,
    author text NOT NULL,
    title text NOT NULL,
    create_time timestamp without time zone NOT NULL,
    replies bigint NOT NULL,
    views bigint NOT NULL,
    last_reply timestamp without time zone NOT NULL,
    section bigint NOT NULL,
    PRIMARY KEY (id)
);
```

#### Scraping Posts List

```sh
./blackhatworld
```

This program is written by Rust with `headless-chrome` (the Rust version of [Puppeteer](https://pptr.dev/)), so you should prepare a Chrome browser before running.

Then you will see a Chrome page open. With a certain probability, the Chrome will popup a Cloudflare verifying page and you should solve it manually or refresh page several times.

<a id="update-mode" name="update-mode"></a>Once you solve it, the remaining process is automatic and it will take about 2 ~ 4 minutes to track newer posts (of course the first time will be extreme longer if your initial database is empty).

[`src/blackhatworld/main.rs`](./src/blackhatworld/main.rs) contains the name of the forums it will scrape. You can change them freely.

#### Scraping Content

##### Introduction

Content scraping is a relatively harder task since we can scrape posts twenty-by-twenty but content scraping can only be done one by one.

Therefore, we use external proxy to improve the efficiency (currently my database contains $1.6 \times 10^5$ posts, about $5\\,\mathrm{GB}$).

First of all, you should run the [posts-list-scraper](#scraping-posts-list), in order to let the following scraper know which posts (with ID) we need.

Then we use a local server technique —— We built a local server to handle the functionality of ``result uploading``, it plays a role as gateway, which means that we can write different kinds of scrapers (in different ways), and all of the data will send to our local server in a simple (and unified) format.
These scrapers can written in various languages and we avoid sticking lower into databases.
The server also has the functions like "load balancing", different scrapers (workers) first request to it to get the disjoint work, and do them themselves, finally upload to server, thus ensures the efficient use of resources.

##### Usage

We can use `./blackhatworld-server` to start the server. The server listen on the UNIX socket [`./underground-scraper.sock`](./underground-scraper.sock) by default and one can forward (like NGINX) it to a TCP port (localhost such as `127.0.0.1`) or directly modify the [code](./src/blackhatworld-server/main.rs#L56)[^1].

[^1]: Anyway, as long as one can access the server in the same manner (TCP port / socket), then it will work. For example, the `blackhatworld-worker` uses the TCP port 18322 in localhost.

Then we can use `GET /get/black` and `POST /send/black` (with JSON `{ id, content }`) to fetch and upload works, and we use `blackhatworld-worker` for sample content scraping.

First we need to configure the proxy in environment variables, they use in compile time similarly:
```sh
export PROXY_HOST=<host, like example.com> # optional, not supplying implies no proxy (dangerous!)
export PROXY_USERNAME=<username> # optional
export PROXY_PASSWORD=<password> # optional
```

To scrape fluently, we should prepare some headers, namely (`Cookie`, `User-Agent`) pairs, one can run
```sh
./blackhatworld-worker config 10001
```
(10001 is the port number of the corresponding proxy) to create a Chrome, and once it pass the Cloudflare verification, we use the Network tool to record their `Cookie` and `User-Agent`, these value can use for a long while (about 1 day).

Finally we create a JSON file, for example `headers.json` with following contents:
```json
{
    "10001": { // port number
        "Cookie": "cf_clearance=...; ...",
        "User-Agent": "Mozilla/5.0 (...) ..."
    },
    "10002": {
        ...
    },
    ...
}
```

Once you've collected enough headers, you can run
```sh
./blackhatworld-worker work headers.json
```
to start formal scraping (it's fascinating!) and checking whether your headers work or not. It takes about 4~6 hours to get 160k data (and it may be faster!).

### Telegram

#### Config file

Since our Telegram Scraper supports parallel scraping in multi-account now, we use config file instead of building environment variables.

See Telegram's [Apps](https://my.telegram.org/apps) page to register and get your `api_id` and `api_hash`.

Then you should create a file named `telegram/config.json` (it can changed in command line arguments in `./telegram`, see `./telegram --help`), with [following content](./telegram/config.json.example):

```json
[
	{
		"id": "api_id_1",
		"hash": "api_hash_1"
	},
	{
		"id": "api_id_2",
		"hash": "api_hash_2",
		"proxy": null
	},
	{
		"id": "api_id_3",
		"hash": "api_hash_3",
		"proxy": "http://www.example.com/"
	},
	{
		"id": "api_id_4",
		"hash": "api_hash_4",
		"proxy": "http://username:password@www.example.com/",
		"session_file": 123456
	},
	...
]
```

#### SQL Schema

```sql
DROP SCHEMA IF EXISTS telegram CASCADE;

CREATE SCHEMA telegram;

CREATE TABLE telegram.channel (
    id bigint NOT NULL,
    name text NOT NULL,
    min_message_id integer NOT NULL,
    max_message_id integer NOT NULL,
    access_hash bigint NOT NULL,
    last_fetch timestamp without time zone NOT NULL,
    app_id integer NOT NULL,
    PRIMARY KEY (id)
);

CREATE TABLE telegram.message (
    id bigint NOT NULL,
    message_id integer NOT NULL,
    channel_id bigint NOT NULL,
    data jsonb NOT NULL,
    PRIMARY KEY (id)
);

CREATE TABLE telegram.link (
    c1 bigint NOT NULL,
    message_id integer NOT NULL,
    c2 bigint NOT NULL,
    PRIMARY KEY (c1, message_id, c2)
);

CREATE TABLE telegram.invite (
    hash text NOT NULL,
    channel_id bigint NOT NULL,
    type "char" NOT NULL,
    description text NOT NULL,
    PRIMARY KEY (hash)
);

CREATE TABLE telegram.bots (
    id bigint NOT NULL,
    name text NOT NULL,
    access_hash bigint NOT NULL,
    app_id integer NOT NULL,
    PRIMARY KEY (id)
);

CREATE TABLE telegram.interaction (
    bot_id bigint NOT NULL,
    message_id integer NOT NULL,
    request text NOT NULL,
    response jsonb NOT NULL,
    PRIMARY KEY (bot_id, message_id)
);

CREATE INDEX ON telegram.channel (lower(name));

CREATE INDEX ON telegram.message (channel_id, message_id);

CREATE INDEX ON telegram.bots (lower(name));
```

#### Usage

First, you should collect channels/groups as many as possible. Run
```sh
./telegram ping -c <channels, both id and username accepted>
```
to add the credentials of the channels to the database.

---

```sh
./telegram content
```
This command is aim to collecting (and updating) the content in corresponding channels, it is also worked as a updating mode, just like [this](#update-mode).

---

```sh
./telegram extract
```
This command is aim to extract in scraped content to extract more Telegram links and do a cycle to further scraping.
