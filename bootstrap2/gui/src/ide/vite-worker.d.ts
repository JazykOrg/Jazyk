// Vite's `?worker` import shape; the project tsconfig does not pull in vite/client.
declare module '*?worker' {
  const workerFactory: new () => Worker
  export default workerFactory
}
