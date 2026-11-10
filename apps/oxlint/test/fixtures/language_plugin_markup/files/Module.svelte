<script module>
  const fromModule = 1;
  const unusedInModule = 1;
</script>

<script>
  let local = fromModule;
</script>

<p>@read:local</p>
