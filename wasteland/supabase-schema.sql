-- Canonical schema for api_tokens. Run in Supabase SQL editor.

drop table if exists api_tokens cascade;

create table api_tokens (
  id uuid primary key default gen_random_uuid(),
  user_id uuid not null references auth.users(id) on delete cascade,
  name text not null,
  token_hash text not null unique,
  prefix text not null,
  scope text not null default 'read-write' check (scope in ('read', 'read-write')),
  created_at timestamptz not null default now(),
  expires_at timestamptz,
  last_used_at timestamptz
);

create index api_tokens_user_id_idx on api_tokens (user_id);

alter table api_tokens enable row level security;

create policy "users read own tokens" on api_tokens for select
  using (auth.uid() = user_id);
create policy "users create own tokens" on api_tokens for insert
  with check (auth.uid() = user_id);
create policy "users delete own tokens" on api_tokens for delete
  using (auth.uid() = user_id);

create view api_tokens_public with (security_invoker = true) as
  select id, user_id, name, prefix, scope, created_at, expires_at, last_used_at
  from api_tokens;

grant select on api_tokens_public to authenticated;
