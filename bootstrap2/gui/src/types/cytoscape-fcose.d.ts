// cytoscape-fcose ships no type declarations; register it as a plain extension.
declare module 'cytoscape-fcose' {
  import cytoscape from 'cytoscape'
  const ext: cytoscape.Ext
  export default ext
}
