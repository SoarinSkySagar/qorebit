Hi, this is Qorebit, a hardware wallet for QoreChain. It runs on an open-source Baochip board with two buttons. The left button means no. The right button means yes. There is no touchscreen and no keyboard, so nothing on my computer can approve anything for me.

This board is brand new. I just reset it, and the first thing it does is make its own wallet. On the website I click "Set the PIN on the device", and now I type my PIN on the board itself. Left is zero, right is one. Eight presses, and then the same eight again to confirm. The board asks for this PIN on every signature.

Next, the recovery words. They are shown only on the board's own screen, never on the computer, so I will not read them out.

This is my account on the QoreChain testnet, funded from the faucet. QoreChain needs every account to have a post-quantum key, so I register it. The board shows me what it is about to sign. I press right, enter my PIN, and it signs.

Now a transfer. I am sending a tiny amount, 0.001 QOR, to my second account. I press "Review on the device". The board decodes the transaction itself, so what its screen shows, the amount, the address and the network, is exactly what it signs. I read it, press right, and enter my PIN.

Every QoreChain transaction needs two signatures: a classical one, secp256k1, and a post-quantum one, ML-DSA-87. The board makes both. And here it is on the explorer. The chain accepted it, with both signatures.

Now I send again, but this time I press left. Left means no. Nothing is signed.

The same board also signs sign-in messages for websites, and QoreChain smart accounts and multisig. Each one shows its own label on the screen, so I always know what I am approving.

Behind this, the Rust signer is tested byte for byte against QoreChain's own QoreSign library, and a real QoreChain node checks what the firmware signs.

The chip and the board are open hardware, designed so that people can inspect them. To our knowledge, this may be the first post-quantum hardware wallet on open, verifiable hardware. It is a research prototype and it has not been audited, so please do not use it with real funds. I built it with Claude Code: I did the research, the design and the testing, and the AI wrote the code, and AI_USAGE.md records exactly who did what.

Thanks for watching.
