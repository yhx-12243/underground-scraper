# Underground Forum Scraper Collection

## Introduction


The whole project is written in [Rust](https://www.rust-lang.org/) (mainly).


## Table of Contents

* [BlackHatWorld](#blackhatworld)
* [AccsMarket](#accsmarket)
* [EZKIFY Services](#ezkify-services)
* [Telegram](#telegram)

## Global Environment Variables

Use [PostgreSQL](https://www.postgresql.org/),

```sh
DB_HOST_PATH=/var/run/postgresql
DB_USER=postgres
DB_NAME=postgres
DB_PASSWORD=<password> # optional
```

## Patches

todo!()

## BlackHatWorld

### SQL Schema

```sql
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

### Scraping Posts List

```sh
./blackhatworld
```

### Scraping Content

We use `work.mjs` file (in [Node.js](https://my.telegram.org/apps)) for content scraping.

Before running this file, you should run
```sh
npm i --no-save puppeteer
```
to install the corresponding requirement for the script.

```sh
./work.mjs 10001
```

```sh
./work.mjs headers.json
```

## AccsMarket

### SQL Schema

```sql
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

### Usage

```sh
./accsmarket
```

## EZKIFY Services

### SQL Schema

```sql
CREATE SCHEMA ezkify;

CREATE TABLE ezkify.categories (
    id bigint NOT NULL,
    "desc" text NOT NULL,
	PRIMARY KEY (id, "time"),
);

CREATE TABLE ezkify.items (
    id bigint NOT NULL,
    "time" timestamp without time zone NOT NULL,
    category_id bigint NOT NULL,
    service text NOT NULL,
    rate_per_1k double precision NOT NULL,
    min_order bigint NOT NULL,
    max_order bigint NOT NULL,
    description text NOT NULL,
	PRIMARY KEY (id, "time"),
	FOREIGN KEY (category_id) REFERENCES ezkify.categories(id)
);
```

### Usage

```sh
./ezkify
```

## Telegram

### Environment Variables

See Telegram's [Apps](https://my.telegram.org/apps) page to register and get your `api_id` and `api_hash`.

```sh
TG_ID=<telegram api_id>
TG_HASH=<telegram api_hash>
```

### SQL Schema

```sql
CREATE SCHEMA telegram;

CREATE TABLE telegram.channel (
    id bigint NOT NULL,
    name text NOT NULL,
    min_message_id integer NOT NULL,
    max_message_id integer NOT NULL,
    access_hash bigint NOT NULL,
	PRIMARY KEY (id)
);

CREATE TABLE telegram.message (
    id bigint NOT NULL,
    message_id integer NOT NULL,
    channel_id bigint NOT NULL,
    data jsonb NOT NULL,
	PRIMARY KEY (id)
);

CREATE INDEX channel_name_lower_idx ON telegram.channel USING btree (lower(name));

CREATE INDEX message_channel_id_message_id_idx ON telegram.message USING btree (channel_id, message_id);
```

### Usage

```sh
./telegram ping <channels, both id and username accepted>
```

```sh
./telegram content
```

```sh
./telegram extract
```
