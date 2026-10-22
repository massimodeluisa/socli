import type { Theme } from 'vitepress'
import Layout from './Layout.vue'
import InstallCommand from './components/InstallCommand.vue'
import DemoReader from './components/DemoReader.vue'
import SoMark from './components/SoMark.vue'

import './styles/site.css'
import './styles/docs.css'
import './styles/design.css'
import './styles/vp.css'

export default {
  Layout,
  enhanceApp({ app }) {
    app.component('InstallCommand', InstallCommand)
    app.component('DemoReader', DemoReader)
    app.component('SoMark', SoMark)
  },
} satisfies Theme
