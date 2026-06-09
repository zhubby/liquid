alter table database_backups
    add column if not exists storage_kind text,
    add column if not exists local_path text;

update database_backups
set storage_kind = 's3'
where storage_kind is null
  and s3_bucket is not null
  and s3_key is not null;

alter table database_backups
    drop constraint if exists database_backups_object_complete_check;

alter table database_backups
    add constraint database_backups_storage_kind_check check (
        storage_kind is null or storage_kind in ('local', 's3')
    ),
    add constraint database_backups_local_path_not_blank check (
        local_path is null or length(trim(local_path)) > 0
    ),
    add constraint database_backups_object_complete_check check (
        (status <> 'succeeded')
        or (
            storage_kind = 'local'
            and local_path is not null
            and length(trim(local_path)) > 0
            and size_bytes is not null
            and size_bytes >= 0
            and checksum_sha256 is not null
            and length(trim(checksum_sha256)) > 0
        )
        or (
            storage_kind = 's3'
            and s3_bucket is not null
            and length(trim(s3_bucket)) > 0
            and s3_key is not null
            and length(trim(s3_key)) > 0
            and size_bytes is not null
            and size_bytes >= 0
            and checksum_sha256 is not null
            and length(trim(checksum_sha256)) > 0
        )
    );
