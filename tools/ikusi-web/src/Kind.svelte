<script lang="ts">
  import { Icon } from '@enekos/osagai';
  import { iconOf, colorOf } from './navigation';
  let { type, small = false }: { type: string; small?: boolean } = $props();
</script>

<span class="kind-icon {colorOf(type)}" class:small><Icon name={iconOf(type)} size={small ? 15 : 18} /></span>
