# Backend system

The backend system is a REST API built using Node.js and Express.
It is backed by a PostgreSQL database and provides endpoints for the [frontend interface](./frontend.md).

# API Endpoints

# Database

The backend system uses a PostgreSQL database to store and manage data related to the warehouse operations.

See the [database documentation](./database/database.md) for more information.

# Sub-systems

There are several sub-systems, each of which is responsible for a specific aspect of the backend
functionality. Each sub-system defines its own [API endpoints](#api-endpoints), [database schema](#database),
and business logic.

The sub-systems are:

- [User Management](./subsystems/user.md)
- [Inventory Management](./subsystems/inventory.md)

More to come soon.
