# User Management

The user management system handles user accounts and authentication.

# Properties

Each user account has the following properties:
- `id` - a unique identifier for the user
- `role` - the role of the user (Admin, Manager, or Staff)
- `username` - the username of the user
- `password` - the MD5 hash password of the user (TODO upgrade this later)

# Operations

The user management system supports the following operations:

## Login operation
- `authenticateUser` - authenticates a user with a username and password

## Usage operation
- `passwordChange` - changes the password of your own account

## Management operations
- `createUser` - creates a new user account
- `updateUser` - updates an existing user account
- `deleteUser` - deletes a user account
- `passwordReset` - resets the password of a user account

# Security

- Login operation can be performed by unauthenticated.
- Usage operation can be performed by all users.
- All management operations can be performed by Admins only.
