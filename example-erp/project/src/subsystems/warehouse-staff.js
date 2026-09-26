function actorRoleFromHeaders(headers = {}) {
  const authorization = String(headers.authorization || headers.Authorization || '');
  const bearerRole = authorization.match(/role:([^\s]+)/i);
  return String(
    headers['x-user-role']
    || headers['X-User-Role']
    || headers['x-role']
    || headers['X-Role']
    || (bearerRole && bearerRole[1])
    || ''
  ).trim().toLowerCase();
}

function mayPerformInventoryOperation(headers = {}) {
  return actorRoleFromHeaders(headers) === 'warehouse-staff';
}

function requireWarehouseStaff(headers = {}) {
  if (!mayPerformInventoryOperation(headers)) {
    const error = new Error('Only warehouse staff may perform inventory operations');
    error.statusCode = 403;
    throw error;
  }
  return true;
}

module.exports = {
  actorRoleFromHeaders,
  mayPerformInventoryOperation,
  requireWarehouseStaff
};
