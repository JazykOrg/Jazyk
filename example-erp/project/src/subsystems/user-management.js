const crypto = require('crypto');

const roles = ['Admin', 'Manager', 'Staff'];
const users = new Map();

function md5(value) {
  return crypto.createHash('md5').update(String(value)).digest('hex');
}

function sanitize(user) {
  return {
    id: user.id,
    role: user.role,
    username: user.username,
    password: user.password
  };
}

const userManagement = {
  apiEndpoints: [
    'POST /users/authenticate',
    'POST /users/passwordChange',
    'POST /users',
    'PUT /users/:id',
    'DELETE /users/:id',
    'POST /users/:id/passwordReset'
  ],
  databaseSchema: {
    users: {
      id: 'text primary key',
      role: 'text check role in (Admin, Manager, Staff)',
      username: 'text unique not null',
      password: 'md5 hash text not null'
    }
  },
  businessLogic: {}
};

function createUser({ id, role, username, password }) {
  if (!id || !roles.includes(role) || !username || !password) {
    throw Object.assign(new Error('id, valid role, username, and password are required'), { statusCode: 400 });
  }
  const account = { id: String(id), role, username: String(username), password: md5(password) };
  users.set(account.id, account);
  return sanitize(account);
}

function updateUser(id, updates) {
  const account = users.get(String(id));
  if (!account) throw Object.assign(new Error('User not found'), { statusCode: 404 });
  if (updates.role) {
    if (!roles.includes(updates.role)) throw Object.assign(new Error('Invalid role'), { statusCode: 400 });
    account.role = updates.role;
  }
  if (updates.username) account.username = String(updates.username);
  return sanitize(account);
}

function deleteUser(id) {
  return users.delete(String(id));
}

function authenticateUser(username, password) {
  const hashed = md5(password);
  return Array.from(users.values()).some((user) => user.username === username && user.password === hashed);
}

function passwordChange(id, currentPassword, nextPassword) {
  const account = users.get(String(id));
  if (!account || account.password !== md5(currentPassword)) {
    throw Object.assign(new Error('Authentication failed'), { statusCode: 403 });
  }
  account.password = md5(nextPassword);
  return sanitize(account);
}

function passwordReset(id, nextPassword) {
  const account = users.get(String(id));
  if (!account) throw Object.assign(new Error('User not found'), { statusCode: 404 });
  account.password = md5(nextPassword);
  return sanitize(account);
}

Object.assign(userManagement, {
  roles,
  userProperties: ['id', 'role', 'username', 'password'],
  createUser,
  updateUser,
  deleteUser,
  authenticateUser,
  passwordChange,
  passwordReset
});

userManagement.businessLogic = {
  createUser,
  updateUser,
  deleteUser,
  authenticateUser,
  passwordChange,
  passwordReset
};

module.exports = userManagement;
