# Local development

The local stack runs PostgreSQL, MinIO, the API, and the worker with Docker Compose. Credentials in
`.env.example` and the Compose defaults are intentionally development-only.

```sh
cp .env.example .env
docker compose up --build
```

The API is available at `http://localhost:8080`, PostgreSQL at `localhost:5432`, MinIO's API at
`http://localhost:9000`, and MinIO's console at `http://localhost:9001`. Each service has a health
check, and the application services wait for both data services to become healthy.

To stop the stack while retaining local data, run `docker compose down`.

## Database migrations

Migration files live in `crates/mc-infrastructure/migrations`. Apply every pending migration with:

```sh
docker compose run --rm api migrate
```

Application code does not create or repair schema implicitly.
