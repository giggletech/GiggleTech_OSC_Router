# Twitch Bot with Supabase-backed Twitch OAuth

## Setup

1. Create a Twitch application in the Twitch Developer Console and note the Client ID and Client Secret.
2. In Supabase, create a table `twitch_tokens` using the SQL below.
3. Create a `.env` file (same directory as `bot.js`) with your values.
4. Install dependencies and run the bot.

## Supabase SQL

```sql
create table if not exists public.twitch_tokens (
  channel text primary key,
  access_token text not null,
  refresh_token text not null,
  expires_at timestamptz not null
);
```

## .env example

```ini
TWITCH_USERNAME=your_bot
TWITCH_CHANNEL=your_channel

# Get from https://dev.twitch.tv/console/apps
TWITCH_CLIENT_ID=
TWITCH_CLIENT_SECRET=

# Supabase project settings
SUPABASE_URL=https://YOUR-PROJECT.supabase.co
# WARNING: Service role key must be kept server-side only
SUPABASE_SERVICE_ROLE_KEY=
SUPABASE_TABLE=twitch_tokens

# Router configuration
ROUTER_HOST=127.0.0.1
ROUTER_PORT=9001
PROX_ADDR=/avatar/parameters/proximity_01
```

## Install and run

```bash
cd twitch-bot
npm install
npm run start
```

## First-time token population

Populate the `twitch_tokens` row for your `TWITCH_CHANNEL` with a valid `refresh_token`, and an `access_token` plus `expires_at` in the near past. The bot will refresh automatically on startup and thereafter.

