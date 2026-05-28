create table if not exists public.twitch_tokens (
	channel text primary key,
	access_token text not null,
	refresh_token text not null,
	expires_at timestamptz not null
);

