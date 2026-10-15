// Module resolve hook: retries a relative import without an extension as ".ts".
export async function resolve(specifier, context, next) {
  try {
    return await next(specifier, context);
  } catch (err) {
    const relative = specifier.startsWith('./') || specifier.startsWith('../');
    if (err?.code === 'ERR_MODULE_NOT_FOUND' && relative && !/\.[cm]?[jt]s$/.test(specifier)) {
      return next(`${specifier}.ts`, context);
    }
    throw err;
  }
}
