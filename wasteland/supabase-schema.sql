-- Run this in Supabase SQL Editor.
-- Creates the api_tokens table + RLS + a public view that hides token_hash.

create table if not exists api_tokens (
  id uuid primary key default gen_random_uuid(),
  user_id uuid not null references auth.users(id) on delete cascade,
  name text not null,
  token_hash text not null unique,
  prefix text not null,
  created_at timestamptz not null default now(),
  last_used_at timestamptz
);

create index if not exists api_tokens_user_id_idx on api_tokens (user_id);

alter table api_tokens enable row level security;

drop policy if exists "users read own tokens" on api_tokens;
drop policy if exists "users create own tokens" on api_tokens;
drop policy if exists "users delete own tokens" on api_tokens;

create policy "users read own tokens" on api_tokens for select
  using (auth.uid() = user_id);
create policy "users create own tokens" on api_tokens for insert
  with check (auth.uid() = user_id);
create policy "users delete own tokens" on api_tokens for delete
  using (auth.uid() = user_id);

-- View clients SELECT from, so token_hash never hits the wire on reads.
create or replace view api_tokens_public with (security_invoker = true) as
  select id, user_id, name, prefix, created_at, last_used_at from api_tokens;

grant select on api_tokens_public to authenticated;
