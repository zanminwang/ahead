# @ahead/postgres

Ahead's PostgreSQL persistence: the framework tables (`migration.sql`), every statement Ahead runs, and a two-method driver interface. Pick the shim for your access tool and pass it as `createBackend({ database })`:

```ts
import { pg } from "@ahead/postgres";        // node-postgres pool
import { prisma } from "@ahead/postgres";    // Prisma client
import { drizzle } from "@ahead/postgres";   // drizzle-orm/node-postgres
```

Any other tool needs `persistence({ transaction, query })`. See the [documentation](../../website/docs/backend/database.md).
